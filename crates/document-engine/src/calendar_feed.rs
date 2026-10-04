use std::collections::{HashMap, HashSet};

use academic_core::models::{AssignmentKind, Course, NewCourse, SyllabusStatus};
use academic_core::repo::syllabus::NewExtraction;
use academic_core::repo::{calendar_feeds, courses, syllabus};
use academic_core::SqlitePool;
use chrono::{NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
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

/// The timezone a deadline should be read in. Every timestamp in a D2L feed
/// is UTC (`...Z`), so a deadline written `20260305T003000Z` is really
/// 7:30 PM on March *4th* for an Eastern-time student — reading the UTC wall
/// clock as a local date silently files late-evening work a day late.
/// Configured rather than hardcoded, defaulting to the owner's campus zone;
/// an unparseable value falls back to that default rather than failing a sync.
pub fn configured_timezone() -> Tz {
    std::env::var("LOCAL_TIMEZONE")
        .ok()
        .and_then(|raw| raw.trim().parse::<Tz>().ok())
        .unwrap_or(chrono_tz::America::New_York)
}

async fn fetch_ics(url: &str) -> Result<String, DocumentError> {
    let response = reqwest::get(url).await.map_err(|e| DocumentError::FeedFetch(e.to_string()))?;
    if !response.status().is_success() {
        return Err(DocumentError::FeedFetch(format!("server returned HTTP {}", response.status())));
    }
    response.text().await.map_err(|e| DocumentError::FeedFetch(e.to_string()))
}

/// Which state change of a coursework item a VEVENT represents. D2L does not
/// emit one event per assignment — it emits one per *transition*, suffixing
/// SUMMARY with the state: "Quiz 1 - Available" when it opens, "Quiz 1 -
/// Availability Ends" when it closes, "Quiz 1 - Due" for the stated
/// deadline. Taking every event at face value turns one quiz into three
/// assignments, and an "Available" event is not a deadline at all — it is
/// the moment work *can start*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum D2lEventState {
    /// An opening time. Never scheduled as a deadline.
    Available,
    /// A hard close — a real deadline, but less authoritative than "Due".
    AvailabilityEnds,
    /// The stated due date: the best deadline D2L gives.
    Due,
    /// No recognised suffix. Scheduled on its own terms, since a feed from
    /// another LMS (or a hand-made calendar) won't use D2L's convention.
    Unknown,
}

impl D2lEventState {
    /// How strongly this state should win when several events describe the
    /// same item. Higher wins.
    fn precedence(self) -> u8 {
        match self {
            D2lEventState::Due => 3,
            D2lEventState::AvailabilityEnds => 2,
            D2lEventState::Unknown => 1,
            D2lEventState::Available => 0,
        }
    }

    fn is_deadline(self) -> bool {
        self != D2lEventState::Available
    }
}

/// Splits "Quiz 1 - Due" into ("Quiz 1", Due). Longest suffix first so
/// "Availability Ends" is never mistaken for part of a title, and a suffix
/// that would leave an empty title is ignored (an item genuinely called
/// "Due" keeps its name).
fn split_event_state(summary: &str) -> (String, D2lEventState) {
    const SUFFIXES: [(&str, D2lEventState); 3] = [
        (" - Availability Ends", D2lEventState::AvailabilityEnds),
        (" - Available", D2lEventState::Available),
        (" - Due", D2lEventState::Due),
    ];
    for (suffix, state) in SUFFIXES {
        if let Some(base) = summary.strip_suffix(suffix) {
            let base = base.trim();
            if !base.is_empty() {
                return (base.to_string(), state);
            }
        }
    }
    (summary.trim().to_string(), D2lEventState::Unknown)
}

/// Identity of the underlying coursework item, independent of which state
/// change D2L happened to emit. This is what makes the "Due" and
/// "Availability Ends" events for one quiz collapse into a single scheduled
/// item — and keeps them collapsed across re-syncs, even though each event
/// carries its own UID and so would otherwise look brand new every time.
pub fn canonical_key(org_unit_id: Option<&str>, base_title: &str) -> String {
    format!("{}|{}", org_unit_id.unwrap_or_default(), base_title.trim().to_lowercase())
}

/// Reverses RFC 5545 text escaping: `\n`/`\N` are newlines, and `\\`, `\,`
/// and `\;` are literal characters. `ical` 0.11 unfolds continuation lines
/// but performs no unescaping, so without this a LOCATION like
/// `CHEM-107 - General Chem I (02\, 934\, X57)` becomes a course named with
/// literal backslashes. Consumed one character at a time so an unescaped
/// backslash is preserved rather than eating the character after it.
fn unescape_ics_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(',') => out.push(','),
            Some(';') => out.push(';'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

struct ParsedEvent {
    uid: Option<String>,
    /// SUMMARY as written, kept for the review screen's provenance trail.
    summary: String,
    /// SUMMARY with D2L's state suffix removed — what the assignment is
    /// actually called.
    base_title: String,
    state: D2lEventState,
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
/// (floating/local), or `20261005` (all-day, DATE value type).
///
/// A trailing `Z` means a real instant, so it is converted into `tz` before
/// the date is taken — otherwise an 00:30Z deadline reads as the following
/// day. Without the `Z` the value is a wall clock already written in some
/// local zone, and is taken as-is rather than guessed at; an all-day DATE
/// has no instant to convert.
fn parse_ics_datetime(raw: &str, tz: Tz) -> (Option<String>, Option<String>) {
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
    let wall_time = format!("{}:{}", &time_digits[0..2], &time_digits[2..4]);
    if !raw.ends_with('Z') {
        return (Some(date), Some(wall_time));
    }
    match NaiveDateTime::parse_from_str(&format!("{date} {wall_time}"), "%Y-%m-%d %H:%M") {
        Ok(naive) => {
            let local = Utc.from_utc_datetime(&naive).with_timezone(&tz);
            (Some(local.format("%Y-%m-%d").to_string()), Some(local.format("%H:%M").to_string()))
        }
        // An impossible date (month 13, day 32) can't be converted, but the
        // raw strings are still better review material than nothing.
        Err(_) => (Some(date), Some(wall_time)),
    }
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

fn parse_events(ics_text: &str, tz: Tz) -> Vec<ParsedEvent> {
    let parser = ical::IcalParser::new(ics_text.as_bytes());
    let mut events = Vec::new();
    for calendar in parser {
        let Ok(calendar) = calendar else { continue };
        for event in calendar.events {
            let summary = prop_value(&event, "SUMMARY").map(|s| unescape_ics_text(&s)).unwrap_or_default();
            if summary.trim().is_empty() {
                continue;
            }
            let (base_title, state) = split_event_state(&summary);
            // DTSTART first, DTEND as a fallback — but only when DTSTART
            // yielded no date at all, so a malformed DTSTART doesn't shadow
            // a perfectly good DTEND.
            let (due_date, due_time) = [prop_value(&event, "DTSTART"), prop_value(&event, "DTEND")]
                .into_iter()
                .flatten()
                .map(|raw| parse_ics_datetime(&raw, tz))
                .find(|(date, _)| date.is_some())
                .unwrap_or((None, None));
            let description = prop_value(&event, "DESCRIPTION").map(|d| unescape_ics_text(&d));
            let org_unit_id = description.as_deref().and_then(extract_org_unit_id);
            events.push(ParsedEvent {
                uid: prop_value(&event, "UID"),
                summary,
                base_title,
                state,
                description,
                location: prop_value(&event, "LOCATION").map(|l| unescape_ics_text(&l)),
                org_unit_id,
                due_date,
                due_time,
            });
        }
    }
    events
}

fn guess_kind(title: &str) -> AssignmentKind {
    let s = title.to_lowercase();
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

/// Which of two events describing the same item should be the one scheduled:
/// the more authoritative state wins, and on a tie the earlier deadline
/// wins so a student is never shown the later of two dates for one piece of
/// work.
fn supersedes(candidate: &ParsedEvent, incumbent: &ParsedEvent) -> bool {
    match candidate.state.precedence().cmp(&incumbent.state.precedence()) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            (candidate.due_date.as_deref(), candidate.due_time.as_deref()) < (incumbent.due_date.as_deref(), incumbent.due_time.as_deref())
        }
    }
}

/// Collapses a feed's events down to one entry per real coursework item:
/// drops openings and undated entries, then keeps the most authoritative
/// event per `canonical_key`.
fn collapse_to_deadlines(events: Vec<ParsedEvent>) -> Vec<(String, ParsedEvent)> {
    let mut by_item: HashMap<String, ParsedEvent> = HashMap::new();
    for event in events {
        if !event.state.is_deadline() || event.due_date.is_none() {
            continue;
        }
        let key = canonical_key(event.org_unit_id.as_deref(), &event.base_title);
        match by_item.get(&key) {
            Some(incumbent) if !supersedes(&event, incumbent) => {}
            _ => {
                by_item.insert(key, event);
            }
        }
    }
    let mut out: Vec<(String, ParsedEvent)> = by_item.into_iter().collect();
    // HashMap order is arbitrary; sort so a sync's inserts are deterministic
    // and the review list arrives in deadline order.
    out.sort_by(|a, b| {
        (&a.1.due_date, &a.1.due_time, &a.1.base_title).cmp(&(&b.1.due_date, &b.1.due_time, &b.1.base_title))
    });
    out
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
/// only ever shows genuinely new courses to confirm. `event_count` counts
/// the deadlines that would actually be scheduled, not raw VEVENTs, so it
/// matches what a sync goes on to create.
pub async fn detect_course_groups(pool: &SqlitePool, feed_id: &str) -> Result<Vec<DetectedCourseGroup>, DocumentError> {
    let feed = calendar_feeds::get(pool, feed_id)
        .await?
        .ok_or_else(|| DocumentError::FeedNotFound(feed_id.to_string()))?;
    let ics_text = fetch_ics(&feed.ics_url).await?;

    let mut groups: HashMap<String, (Option<String>, String, usize)> = HashMap::new();
    for (_, event) in collapse_to_deadlines(parse_events(&ics_text, configured_timezone())) {
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
/// deadline as a pending extraction — same review-before-scheduling pipeline
/// the AI syllabus path uses (see `academic_core::repo::syllabus::approve_extraction`).
///
/// Three things are filtered out before anything is stored: undated entries
/// (D2L calendars include non-deadline items like class meeting times),
/// "Available" openings, and sibling events for an item already imported —
/// matched both on exact UID and on `canonical_key`, since D2L gives each of
/// an item's state-change events its own UID.
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

    let known = syllabus::known_feed_items(pool, feed_id).await?;
    let known_uids: HashSet<&str> = known.iter().filter_map(|k| k.external_uid.as_deref()).collect();
    // Derived the same way for stored rows as for incoming events, so items
    // imported before this collapsing existed (whose titles still carry a
    // " - Due" suffix) are still recognised.
    let known_keys: HashSet<String> = known
        .iter()
        .map(|k| canonical_key(k.external_org_unit_id.as_deref(), &split_event_state(&k.title).0))
        .collect();

    let all_courses = courses::list_all(pool).await?;
    let courses_by_org_unit: HashMap<&str, &Course> =
        all_courses.iter().filter_map(|c| c.external_org_unit_id.as_deref().map(|id| (id, c))).collect();

    let new_items: Vec<NewExtraction> = collapse_to_deadlines(parse_events(&ics_text, configured_timezone()))
        .into_iter()
        .filter(|(key, event)| !known_keys.contains(key) && !event.uid.as_deref().is_some_and(|u| known_uids.contains(u)))
        .map(|(_, e)| {
            let linked_course = e.org_unit_id.as_deref().and_then(|id| courses_by_org_unit.get(id).copied());
            let (course, confidence) = match linked_course {
                Some(c) => (Some(c), 0.98),
                None => {
                    let haystack = format!("{} {}", e.location.clone().unwrap_or_default(), e.base_title);
                    match guess_course_by_text(&haystack, &all_courses) {
                        Some(c) => (Some(c), 0.9),
                        None => (None, 0.6),
                    }
                }
            };
            NewExtraction {
                kind: guess_kind(&e.base_title),
                title: e.base_title.clone(),
                description: e.description.clone(),
                due_date: e.due_date.clone(),
                due_time: e.due_time.clone(),
                // The untouched SUMMARY, so the review screen shows which
                // of D2L's state events this deadline actually came from.
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

    const EASTERN: Tz = chrono_tz::America::New_York;

    #[test]
    fn normalizes_webcal_scheme() {
        assert_eq!(normalize_ics_url("webcal://d2l.school.edu/feed.ics"), "https://d2l.school.edu/feed.ics");
        assert_eq!(normalize_ics_url("https://d2l.school.edu/feed.ics"), "https://d2l.school.edu/feed.ics");
    }

    #[test]
    fn converts_utc_instant_into_local_date() {
        // 20261005T235900Z is 7:59 PM Eastern on the *same* day.
        assert_eq!(parse_ics_datetime("20261005T235900Z", EASTERN), (Some("2026-10-05".into()), Some("19:59".into())));
    }

    #[test]
    fn utc_after_midnight_lands_on_the_previous_local_day() {
        // The regression this exists for: a 00:30Z deadline is 7:30 PM the
        // evening *before* in Eastern time, and used to be filed a day late.
        assert_eq!(parse_ics_datetime("20260305T003000Z", EASTERN), (Some("2026-03-04".into()), Some("19:30".into())));
    }

    #[test]
    fn utc_timezone_leaves_the_wall_clock_alone() {
        assert_eq!(parse_ics_datetime("20261005T235900Z", chrono_tz::UTC), (Some("2026-10-05".into()), Some("23:59".into())));
    }

    #[test]
    fn floating_time_is_taken_as_written() {
        // No trailing Z: already a local wall clock, so it must not shift.
        assert_eq!(parse_ics_datetime("20261005T235900", EASTERN), (Some("2026-10-05".into()), Some("23:59".into())));
    }

    #[test]
    fn parses_date_only() {
        assert_eq!(parse_ics_datetime("20261005", EASTERN), (Some("2026-10-05".into()), None));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_ics_datetime("not-a-date", EASTERN), (None, None));
    }

    #[test]
    fn guesses_kind_from_title() {
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
    fn unescapes_ics_text() {
        assert_eq!(unescape_ics_text("General Chem I (02\\, 934\\, X57)"), "General Chem I (02, 934, X57)");
        assert_eq!(unescape_ics_text("Grades:\\nHomework 1"), "Grades:\nHomework 1");
        assert_eq!(unescape_ics_text("a\\;b"), "a;b");
        assert_eq!(unescape_ics_text("path\\\\to"), "path\\to");
    }

    #[test]
    fn leaves_unknown_escape_sequences_intact() {
        assert_eq!(unescape_ics_text("C:\\temp"), "C:\\temp");
        assert_eq!(unescape_ics_text("trailing\\"), "trailing\\");
    }

    #[test]
    fn splits_d2l_state_suffixes() {
        assert_eq!(split_event_state("Quiz 1 - Due"), ("Quiz 1".to_string(), D2lEventState::Due));
        assert_eq!(split_event_state("Quiz 1 - Available"), ("Quiz 1".to_string(), D2lEventState::Available));
        assert_eq!(
            split_event_state("Quiz 1 - Availability Ends"),
            ("Quiz 1".to_string(), D2lEventState::AvailabilityEnds)
        );
        assert_eq!(split_event_state("Reading Day"), ("Reading Day".to_string(), D2lEventState::Unknown));
    }

    #[test]
    fn keeps_a_title_that_would_be_emptied_by_stripping() {
        // Stripping would leave nothing, so the suffix is kept as the title
        // (trimmed, like every other title) rather than producing a blank.
        assert_eq!(split_event_state(" - Due"), ("- Due".to_string(), D2lEventState::Unknown));
    }

    #[test]
    fn canonical_key_is_state_and_case_independent() {
        let due = split_event_state("Quiz 1 - Due").0;
        let ends = split_event_state("quiz 1 - Availability Ends").0;
        assert_eq!(canonical_key(Some("4066089"), &due), canonical_key(Some("4066089"), &ends));
    }

    #[test]
    fn canonical_key_separates_identical_titles_in_different_courses() {
        assert_ne!(canonical_key(Some("4066089"), "Quiz 1"), canonical_key(Some("4148085"), "Quiz 1"));
    }

    #[test]
    fn parses_minimal_ics_calendar() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:abc-123\r\nSUMMARY:CS301 Homework 1\r\nDTSTART:20261005T235900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_events(ics, EASTERN);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "CS301 Homework 1");
        assert_eq!(events[0].uid.as_deref(), Some("abc-123"));
        assert_eq!(events[0].due_date.as_deref(), Some("2026-10-05"));
    }

    #[test]
    fn parses_location_and_org_unit_from_real_shaped_event() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:6606-1@sru.desire2learn.com\r\nSUMMARY:Hwk1 - Due\r\nLOCATION:CHEM-107 - General Chem I (02\\, 934)\r\nDESCRIPTION:View event - https://sru.desire2learn.com/d2l/le/calendar/4066089/event/1/detailsview?ou=4066089#1\r\nDTSTART:20261005T235900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_events(ics, EASTERN);
        assert_eq!(events[0].location.as_deref(), Some("CHEM-107 - General Chem I (02, 934)"));
        assert_eq!(events[0].org_unit_id.as_deref(), Some("4066089"));
        assert_eq!(events[0].base_title, "Hwk1");
        assert_eq!(events[0].state, D2lEventState::Due);
    }

    /// The exact shape the live SRU feed produced: one quiz emitted as three
    /// VEVENTs with three different UIDs.
    fn three_state_quiz_ics() -> String {
        let event = |uid: &str, summary: &str, dtstart: &str| {
            format!(
                "BEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{summary}\r\nLOCATION:MATH-225-02 - Calculus I\r\nDESCRIPTION:View event - https://sru.desire2learn.com/d2l/le/calendar/4148085/event/1/detailsview?ou=4148085#1\r\nDTSTART:{dtstart}\r\nEND:VEVENT\r\n"
            )
        };
        format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{}{}{}END:VCALENDAR\r\n",
            event("6606-1@sru", "Quiz 1 - Available", "20260901T040000Z"),
            event("6606-2@sru", "Quiz 1 - Availability Ends", "20260908T035900Z"),
            event("6606-3@sru", "Quiz 1 - Due", "20260907T035900Z"),
        )
    }

    #[test]
    fn three_state_events_collapse_to_one_due_deadline() {
        let collapsed = collapse_to_deadlines(parse_events(&three_state_quiz_ics(), EASTERN));
        assert_eq!(collapsed.len(), 1, "one quiz must not become three assignments");
        let (_, event) = &collapsed[0];
        assert_eq!(event.base_title, "Quiz 1");
        assert_eq!(event.state, D2lEventState::Due);
        // 20260907T035900Z -> 11:59 PM Eastern on the 6th.
        assert_eq!(event.due_date.as_deref(), Some("2026-09-06"));
        assert_eq!(event.due_time.as_deref(), Some("23:59"));
        assert_eq!(event.uid.as_deref(), Some("6606-3@sru"));
    }

    #[test]
    fn an_opening_only_item_is_never_scheduled() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Week 1 Aug. 24-28 - Available\r\nDTSTART:20260824T040000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert!(collapse_to_deadlines(parse_events(ics, EASTERN)).is_empty());
    }

    #[test]
    fn undated_events_are_skipped() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Office Hours - Due\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert!(collapse_to_deadlines(parse_events(ics, EASTERN)).is_empty());
    }

    #[test]
    fn availability_ends_is_scheduled_when_no_due_event_exists() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Dry Lab 5 - Availability Ends\r\nDTSTART:20260305T035900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let collapsed = collapse_to_deadlines(parse_events(ics, EASTERN));
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].1.base_title, "Dry Lab 5");
        assert_eq!(collapsed[0].1.state, D2lEventState::AvailabilityEnds);
    }

    #[test]
    fn equal_states_keep_the_earlier_deadline() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:late\r\nSUMMARY:Hwk2 - Due\r\nDTSTART:20260920T035900Z\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:early\r\nSUMMARY:Hwk2 - Due\r\nDTSTART:20260910T035900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let collapsed = collapse_to_deadlines(parse_events(ics, EASTERN));
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].1.uid.as_deref(), Some("early"));
    }

    #[test]
    fn collapsed_output_is_in_deadline_order() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Second - Due\r\nDTSTART:20260920T035900Z\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:First - Due\r\nDTSTART:20260910T035900Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let collapsed = collapse_to_deadlines(parse_events(ics, EASTERN));
        let titles: Vec<&str> = collapsed.iter().map(|(_, e)| e.base_title.as_str()).collect();
        assert_eq!(titles, vec!["First", "Second"]);
    }

    /// Items imported before state-collapsing existed still carry the
    /// " - Due" suffix in their stored title; deriving their key the same
    /// way is what stops a re-sync re-importing all of them.
    #[test]
    fn legacy_stored_titles_derive_the_same_key_as_new_events() {
        let stored = canonical_key(Some("4148085"), &split_event_state("Quiz 1 - Due").0);
        let incoming = collapse_to_deadlines(parse_events(&three_state_quiz_ics(), EASTERN));
        assert_eq!(incoming[0].0, stored);
    }
}

