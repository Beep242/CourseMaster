use academic_core::models::{StudyGuide, StudyGuideKind};
use academic_core::repo::{courses, study_guides};
use academic_core::SqlitePool;
use ai_engine::{AiProvider, CompletionRequest};

use crate::error::DocumentError;

fn kind_instructions(kind: StudyGuideKind) -> &'static str {
    match kind {
        StudyGuideKind::QuickReview => {
            "Write a QUICK REVIEW: a short, high-value summary a student can read in under 5 minutes — just the most \
             important concepts and definitions, as a tight bulleted list."
        }
        StudyGuideKind::Complete => {
            "Write a COMPLETE STUDY GUIDE: thorough explanations of every major concept and topic for this course, \
             organized with clear headings, detailed enough that a student could learn the material from this alone."
        }
        StudyGuideKind::CramSheet => {
            "Write an EXAM CRAM SHEET: only the highest-priority facts and concepts most likely to be tested — \
             extremely dense, no filler, formatted for a last-minute review right before an exam."
        }
        StudyGuideKind::FormulaSheet => {
            "Write a FORMULA SHEET: every key formula/equation relevant to this course, each with a one-line note on \
             what it's for and when to use it. If this subject genuinely has no formulas (e.g. a writing or history \
             course), say so plainly instead of inventing any."
        }
    }
}

const SYSTEM_PROMPT: &str = "You are an expert tutor writing study material for a college student. Use clear Markdown \
formatting (headings, bullet points, bold for key terms). Where the student has pasted their own notes or syllabus \
text below, prioritize and reference that material directly rather than relying only on general knowledge. Drawing on \
general subject-matter knowledge to explain concepts is expected and encouraged — but never state a specific fact \
about THIS course (a date, a policy, a professor's requirement, a number) as if it came from the student's own \
material unless it was actually provided to you.";

/// Unlike syllabus extraction, this is generative content, not a claim about
/// what a specific document says — so leaning on the model's own subject
/// knowledge is the point, not a hallucination risk. The guardrail here is
/// narrower: don't invent *course-specific* specifics (deadlines, policies)
/// that weren't actually given.
pub async fn generate_study_guide(
    pool: &SqlitePool,
    ai: &dyn AiProvider,
    course_id: &str,
    kind: StudyGuideKind,
    material: Option<&str>,
) -> Result<StudyGuide, DocumentError> {
    let course = courses::get(pool, course_id).await?.ok_or_else(|| DocumentError::CourseNotFound(course_id.to_string()))?;

    let mut prompt = format!(
        "Course: {}{}\n\n{}",
        course.name,
        course.code.as_ref().map(|c| format!(" ({c})")).unwrap_or_default(),
        kind_instructions(kind)
    );
    if let Some(material) = material.filter(|m| !m.trim().is_empty()) {
        prompt.push_str("\n\n---STUDENT-PROVIDED MATERIAL TO FOCUS ON---\n");
        prompt.push_str(material);
    }
    prompt.push_str(
        "\n\nRespond with the guide content only, in Markdown, starting directly with a top-level heading naming the guide \
         — no other preamble.",
    );

    let response = ai.complete(CompletionRequest { system_prompt: Some(SYSTEM_PROMPT.to_string()), prompt, effort: None }).await?;
    let content = response.text.trim().to_string();
    if content.is_empty() {
        return Err(DocumentError::EmptyGeneration("study guide".into()));
    }

    let title = content
        .lines()
        .next()
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("{} — {}", course.name, kind.as_str().replace('_', " ")));

    Ok(study_guides::create(pool, course_id, kind, &title, &content).await?)
}
