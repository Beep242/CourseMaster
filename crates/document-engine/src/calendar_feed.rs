use std::collections::HashSet;

use academic_core::models::{AssignmentKind, Course, SyllabusStatus};
use academic_core::repo::syllabus::NewExtraction;
use academic_core::repo::{calendar_feeds, courses, syllabus};
use academic_core::SqlitePool;
use ical::parser::ical::component::IcalEvent;

use crate::error::DocumentError;

/// `webcal://` is just a UI convention meaning "open in a calendar app" —
/// the resource itself is served over plain HTTPS at the same host/path.
/// D2L's own "Subscribe" button hands out `webcal://` URLs, so this needs
/// normalizing before `reqwest` (which has no concept of that scheme) can
/// fetch it.
pub fn normalize_ics_url(url: &str) -> String {
    match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

async fn fetch_ics(url: &str) -> Result<String, DocumentError> {
    let response = reqwest::get(url).await.map_err(|e| DocumentError::FeedFetch(e.to_string()))?;
    if !response.status().is_success() {
        return Err(DocumentError::FeedFetch(format!("server returned HTTP {}", response.status())));
    }
    response.text().await.map_err(|e| DocumentError::FeedFetch(e.to_string()))
}

struct ParsedEvent {
    uid: Option<String>,
    summary: String,
    description: Option<String>,
    due_date: Option<String>,
    due_time: Option<String>,
}

fn prop_value(event: &IcalEvent, name: &str) -> Option<String> {
    event.properties.iter().find(|p| p.name == name).and_then(|p| p.value.clone())
}

/// ICS datetimes look like `20261005T235900Z` (UTC), `20261005T235900`
/// (floating/local), or `20261005` (all-day, DATE value type) — this reads
/// the wall-clock date/time as written rather than resolving `TZID`
/// parameters against a timezone database. A syllabus/feed date is a
/// calendar date to a student regardless of which zone it was authored in,
/// and the review step catches anything that lands wrong.
fn parse_ics_datetime(raw: &str) -> (Option<String>, Option<String>) {
    let raw = raw.trim();
    if raw.len() < 8 || !raw.as_bytes()[..8].iter().all(u8::is_ascii_digit) {
        return (None, None);
    }
    let date = format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]);
    let Some(t_idx) = raw.find('T') else {
        return (Some(date), None);
    };
    let time_digits: String = raw[t_idx + 1..].chars().take_while(char::is_ascii_digit).collect();
    if time_digits.len() < 4 {
        return (Some(date), None);
    }
    (Some(date), Some(format!("{}:{}", &time_digits[0..2], &time_digits[2..4])))
}

fn parse_events(ics_text: &str) -> Vec<ParsedEvent> {
    let parser = ical::IcalParser::new(ics_text.as_bytes());
    let mut events = Vec::new();
    for calendar in parser {
        let Ok(calendar) = calendar else { continue };
        for event in calendar.events {
            let summary = prop_value(&event, "SUMMARY").unwrap_or_default();
            if summary.trim().is_empty() {
                continue;
            }
            let dtstart = prop_value(&event, "DTSTART").as_deref().map(parse_ics_datetime);
            let dtend = prop_value(&event, "DTEND").as_deref().map(parse_ics_datetime);
            let (due_date, due_time) = dtstart.or(dtend).unwrap_or((None, None));
            events.push(ParsedEvent {
                uid: prop_value(&event, "UID"),
                summary,
                description: prop_value(&event, "DESCRIPTION"),
                due_date,
                due_time,
            });
        }
    }
    events
}

fn guess_kind(summary: &str) -> AssignmentKind {
    let s = summary.to_lowercase();
    if s.contains("exam") || s.contains("midterm") || s.contains("final") {
        AssignmentKind::Exam
    } else if s.contains("quiz") {
        AssignmentKind::Quiz
    } else if s.contains("project") {
        AssignmentKind::Project
    } else {
        AssignmentKind::Assignment
    }
}

fn guess_course<'a>(text: &str, courses: &'a [Course]) -> Option<&'a Course> {
    let lower = text.to_lowercase();
    courses
        .iter()
        .find(|c| c.code.as_ref().is_some_and(|code| lower.contains(&code.to_lowercase())) || lower.contains(&c.name.to_lowercase()))
}

/// Fetches and parses a calendar feed, storing every not-already-seen dated
/// event as a pending extraction — same review-before-scheduling pipeline
/// the AI syllabus path uses (see `academic_core::repo::syllabus::approve_extraction`).
/// Undated entries (D2L calendars include non-deadline items like class
/// meeting times) are skipped outright since there's nothing to schedule.
pub async fn sync_feed(pool: &SqlitePool, feed_id: &str) -> Result<usize, DocumentError> {
    let feed = calendar_feeds::get(pool, feed_id)
        .await?
        .ok_or_else(|| DocumentError::FeedNotFound(feed_id.to_string()))?;

    let ics_text = match fetch_ics(&feed.ics_url).await {
        Ok(text) => text,
        Err(err) => {
            calendar_feeds::record_sync_result(pool, feed_id, Some(&err.to_string())).await?;
            return Err(err);
        }
    };

    let known_uids: HashSet<String> = syllabus::known_external_uids_for_feed(pool, feed_id).await?.into_iter().collect();
    let all_courses = courses::list_all(pool).await?;

    let new_items: Vec<NewExtraction> = parse_events(&ics_text)
        .into_iter()
        .filter(|e| e.due_date.is_some())
        .filter(|e| !e.uid.as_ref().is_some_and(|u| known_uids.contains(u)))
        .map(|e| {
            let haystack = format!("{} {}", e.summary, e.description.clone().unwrap_or_default());
            let course = guess_course(&haystack, &all_courses);
            NewExtraction {
                kind: guess_kind(&e.summary),
                title: e.summary.clone(),
                description: e.description.clone(),
                due_date: e.due_date.clone(),
                due_time: e.due_time.clone(),
                source_excerpt: e.summary.clone(),
                confidence: if course.is_some() { 0.9 } else { 0.6 },
                course_id: course.map(|c| c.id.clone()),
                external_uid: e.uid.clone(),
            }
        })
        .collect();

    let count = new_items.len();
    let batch = syllabus::create_calendar_batch(pool, feed_id, &feed.ics_url).await?;
    if count > 0 {
        syllabus::insert_extractions(pool, &batch.id, new_items).await?;
    }
    syllabus::set_status(pool, &batch.id, SyllabusStatus::ReadyForReview, None).await?;
    calendar_feeds::record_sync_result(pool, feed_id, None).await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_webcal_scheme() {
        assert_eq!(normalize_ics_url("webcal://d2l.school.edu/feed.ics"), "https://d2l.school.edu/feed.ics");
        assert_eq!(normalize_ics_url("https://d2l.school.edu/feed.ics"), "https://d2l.school.edu/feed.ics");
    }

    #[test]
    fn parses_utc_datetime() {
        assert_eq!(parse_ics_datetime("20261005T235900Z"), (Some("2026-10-05".into()), Some("23:59".into())));
    }

    #[test]
    fn parses_date_only() {
        assert_eq!(parse_ics_datetime("20261005"), (Some("2026-10-05".into()), None));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_ics_datetime("not-a-date"), (None, None));
    }

    #[test]
    fn guesses_kind_from_summary() {
        assert_eq!(guess_kind("Midterm Exam 1"), AssignmentKind::Exam);
        assert_eq!(guess_kind("Quiz 3"), AssignmentKind::Quiz);
        assert_eq!(guess_kind("Term Project Proposal"), AssignmentKind::Project);
        assert_eq!(guess_kind("Homework 2"), AssignmentKind::Assignment);
    }

    #[test]
    fn parses_minimal_ics_calendar() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:abc-123\r\nSUMMARY:CS301 Homework 1\r\nDTSTART:20261005T235900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_events(ics);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "CS301 Homework 1");
        assert_eq!(events[0].uid.as_deref(), Some("abc-123"));
        assert_eq!(events[0].due_date.as_deref(), Some("2026-10-05"));
    }
}
