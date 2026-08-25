use std::collections::{HashMap, HashSet};

use academic_core::models::{AssignmentKind, Course, NewCourse, SyllabusStatus};
use academic_core::repo::syllabus::NewExtraction;
use academic_core::repo::{calendar_feeds, courses, syllabus};
use academic_core::SqlitePool;
use ical::parser::ical::component::IcalEvent;
use serde::Serialize;

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
    /// D2L's per-event `LOCATION` carries the actual course/section name
    /// (e.g. "CHEM-107 - General Chem I (...)") — far more reliable for
    /// course identification than the free-text SUMMARY.
    location: Option<String>,
    /// The LMS's own org-unit id for the course this event belongs to,
    /// scraped out of the "View event" link D2L embeds in DESCRIPTION
    /// (`...?ou=4066089`). Exact, stable, and — critically — not something
    /// that has to be guessed from title text.
    org_unit_id: Option<String>,
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

/// D2L's DESCRIPTION embeds a "View event" link like
/// `.../calendar/4066089/event/40524716/detailsview?ou=4066089#...` — the
/// `ou=` query param is the org unit id. Deliberately not the leading
/// number in the event UID (`6606-40524716@...`): that prefix turned out to
/// be constant across every course in a real feed (a per-subscription id,
/// not a per-course one) — confirmed against a live feed before relying on
/// this instead.
fn extract_org_unit_id(description: &str) -> Option<String> {
    let idx = description.find("ou=")?;
    let digits: String = description[idx + 3..].chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn strip_trailing_parenthetical(s: &str) -> String {
    let trimmed = s.trim();
    match trimmed.rfind('(') {
        Some(idx) if trimmed.ends_with(')') => trimmed[..idx].trim().to_string(),
        _ => trimmed.to_string(),
    }
}

/// Splits a LOCATION like "CHEM-107 - General Chem I (02, 934)" into a
/// course code and a display name. The segment before " - " only counts as
/// a code if it has both a letter and a digit (so "Fall 2026 Academic
/// Success Launchpad", which has no " - " at all, or a plain room name,
/// falls back to using the whole string as the name instead of guessing a
/// nonsense code) — still just a suggestion the student confirms or edits.
fn suggest_course_from_location(location: &str) -> (Option<String>, String) {
    if let Some(idx) = location.find(" - ") {
        let code_candidate = location[..idx].trim();
        let looks_like_code = code_candidate.chars().any(|c| c.is_ascii_alphabetic()) && code_candidate.chars().any(|c| c.is_ascii_digit());
        if looks_like_code {
            let name = strip_trailing_parenthetical(&location[idx + 3..]);
            return (Some(code_candidate.to_string()), name);
        }
    }
    (None, strip_trailing_parenthetical(location))
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
            let description = prop_value(&event, "DESCRIPTION");
            let org_unit_id = description.as_deref().and_then(extract_org_unit_id);
            events.push(ParsedEvent {
                uid: prop_value(&event, "UID"),
                summary,
                description,
                location: prop_value(&event, "LOCATION"),
                org_unit_id,
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

/// Fuzzy fallback for events whose org unit isn't linked to a course yet —
/// matches LOCATION/SUMMARY text against known course names/codes. Once a
/// course is linked via `link_course_from_group`, matching for that org
/// unit becomes exact (see `sync_feed`) and this is only needed for courses
/// the student hasn't connected yet.
fn guess_course_by_text<'a>(text: &str, courses: &'a [Course]) -> Option<&'a Course> {
    let lower = text.to_lowercase();
    courses
        .iter()
        .find(|c| c.code.as_ref().is_some_and(|code| lower.contains(&code.to_lowercase())) || lower.contains(&c.name.to_lowercase()))
}

#[derive(Debug, Serialize)]
pub struct DetectedCourseGroup {
    pub org_unit_id: Option<String>,
    pub location: String,
    pub suggested_code: Option<String>,
    pub suggested_name: String,
    pub event_count: usize,
}

/// Fetches the feed fresh and groups its events by org unit (falling back
/// to the raw LOCATION string when an event has no `ou=` id), returning
/// only groups that aren't already linked to a course — so the review UI
/// only ever shows genuinely new courses to confirm.
pub async fn detect_course_groups(pool: &SqlitePool, feed_id: &str) -> Result<Vec<DetectedCourseGroup>, DocumentError> {
    let feed = calendar_feeds::get(pool, feed_id)
        .await?
        .ok_or_else(|| DocumentError::FeedNotFound(feed_id.to_string()))?;
    let ics_text = fetch_ics(&feed.ics_url).await?;

    let mut groups: HashMap<String, (Option<String>, String, usize)> = HashMap::new();
    for event in parse_events(&ics_text) {
        let Some(location) = event.location else { continue };
        let key = event.org_unit_id.clone().unwrap_or_else(|| location.clone());
        let entry = groups.entry(key).or_insert_with(|| (event.org_unit_id.clone(), location.clone(), 0));
        entry.2 += 1;
    }

    let mut out = Vec::new();
    for (org_unit_id, location, event_count) in groups.into_values() {
        if let Some(org_unit_id) = &org_unit_id {
            if courses::find_by_org_unit_id(pool, org_unit_id).await?.is_some() {
                continue;
            }
        }
        let (suggested_code, suggested_name) = suggest_course_from_location(&location);
        out.push(DetectedCourseGroup { org_unit_id, location, suggested_code, suggested_name, event_count });
    }
    out.sort_by(|a, b| b.event_count.cmp(&a.event_count));
    Ok(out)
}

/// Creates a course from a detected group and links it to the org unit so
/// every future sync matches this course exactly instead of guessing —
/// then backfills any extraction from an earlier sync that was already
/// sitting pending review for this org unit with no course assigned.
pub async fn link_course_from_group(
    pool: &SqlitePool,
    semester_id: &str,
    org_unit_id: Option<&str>,
    name: &str,
    code: Option<&str>,
) -> Result<(Course, u64), DocumentError> {
    let course = courses::create(
        pool,
        NewCourse {
            semester_id: semester_id.to_string(),
            name: name.to_string(),
            code: code.map(str::to_string),
            professor_name: None,
            professor_email: None,
            credit_hours: None,
            color: None,
            external_org_unit_id: org_unit_id.map(str::to_string),
        },
    )
    .await?;

    let backfilled = match org_unit_id {
        Some(id) => syllabus::backfill_course_for_org_unit(pool, id, &course.id).await?,
        None => 0,
    };
    Ok((course, backfilled))
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
    let courses_by_org_unit: HashMap<&str, &Course> =
        all_courses.iter().filter_map(|c| c.external_org_unit_id.as_deref().map(|id| (id, c))).collect();

    let new_items: Vec<NewExtraction> = parse_events(&ics_text)
        .into_iter()
        .filter(|e| e.due_date.is_some())
        .filter(|e| !e.uid.as_ref().is_some_and(|u| known_uids.contains(u)))
        .map(|e| {
            let linked_course = e.org_unit_id.as_deref().and_then(|id| courses_by_org_unit.get(id).copied());
            let (course, confidence) = match linked_course {
                Some(c) => (Some(c), 0.98),
                None => {
                    let haystack = format!("{} {}", e.location.clone().unwrap_or_default(), e.summary);
                    match guess_course_by_text(&haystack, &all_courses) {
                        Some(c) => (Some(c), 0.9),
                        None => (None, 0.6),
                    }
                }
            };
            NewExtraction {
                kind: guess_kind(&e.summary),
                title: e.summary.clone(),
                description: e.description.clone(),
                due_date: e.due_date.clone(),
                due_time: e.due_time.clone(),
                source_excerpt: e.summary.clone(),
                confidence,
                course_id: course.map(|c| c.id.clone()),
                external_uid: e.uid.clone(),
                external_org_unit_id: e.org_unit_id.clone(),
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
    fn extracts_org_unit_id_from_description() {
        let desc = "Grades:\nHomework 1\n\n\nView event - https://sru.desire2learn.com/d2l/le/calendar/4066089/event/40524716/detailsview?ou=4066089#40524716";
        assert_eq!(extract_org_unit_id(desc), Some("4066089".to_string()));
    }

    #[test]
    fn no_org_unit_id_when_absent() {
        assert_eq!(extract_org_unit_id("just some text with no link"), None);
    }

    #[test]
    fn suggests_code_and_name_from_location_with_dash() {
        let (code, name) = suggest_course_from_location("CHEM-107 - General Chem I (02, 934, 956, 957, X57)");
        assert_eq!(code.as_deref(), Some("CHEM-107"));
        assert_eq!(name, "General Chem I");
    }

    #[test]
    fn suggests_code_for_location_with_extra_dash_segments() {
        let (code, name) = suggest_course_from_location("MATH-225-02 - Calculus I");
        assert_eq!(code.as_deref(), Some("MATH-225-02"));
        assert_eq!(name, "Calculus I");
    }

    #[test]
    fn falls_back_to_whole_string_when_no_code_pattern() {
        let (code, name) = suggest_course_from_location("Fall 2026 Academic Success Launchpad");
        assert_eq!(code, None);
        assert_eq!(name, "Fall 2026 Academic Success Launchpad");
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

    #[test]
    fn parses_location_and_org_unit_from_real_shaped_event() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:6606-1@sru.desire2learn.com\r\nSUMMARY:Hwk1 - Due\r\nLOCATION:CHEM-107 - General Chem I (02)\r\nDESCRIPTION:View event - https://sru.desire2learn.com/d2l/le/calendar/4066089/event/1/detailsview?ou=4066089#1\r\nDTSTART:20261005T235900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_events(ics);
        assert_eq!(events[0].location.as_deref(), Some("CHEM-107 - General Chem I (02)"));
        assert_eq!(events[0].org_unit_id.as_deref(), Some("4066089"));
    }
}
