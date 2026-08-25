use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{PracticeAttempt, PracticeDifficulty, PracticeQuestion, PracticeTest, QuestionKind};

const TEST_COLUMNS: &str = "id, course_id, title, difficulty, created_at";
const QUESTION_COLUMNS: &str = "id, practice_test_id, order_index, kind, topic, question_text, options_json, correct_answer, explanation";
const ATTEMPT_COLUMNS: &str = "id, practice_test_id, score_percentage, answers_json, completed_at";

fn row_to_test(r: sqlx::sqlite::SqliteRow) -> Result<PracticeTest, CoreError> {
    Ok(PracticeTest {
        id: r.try_get("id")?,
        course_id: r.try_get("course_id")?,
        title: r.try_get("title")?,
        difficulty: PracticeDifficulty::parse(&r.try_get::<String, _>("difficulty")?),
        created_at: r.try_get("created_at")?,
    })
}

fn row_to_question(r: sqlx::sqlite::SqliteRow) -> Result<PracticeQuestion, CoreError> {
    let options_json: Option<String> = r.try_get("options_json")?;
    Ok(PracticeQuestion {
        id: r.try_get("id")?,
        practice_test_id: r.try_get("practice_test_id")?,
        order_index: r.try_get("order_index")?,
        kind: QuestionKind::parse(&r.try_get::<String, _>("kind")?),
        topic: r.try_get("topic")?,
        question_text: r.try_get("question_text")?,
        options: options_json.and_then(|s| serde_json::from_str(&s).ok()),
        correct_answer: r.try_get("correct_answer")?,
        explanation: r.try_get("explanation")?,
    })
}

fn row_to_attempt(r: sqlx::sqlite::SqliteRow) -> Result<PracticeAttempt, CoreError> {
    let answers_json: String = r.try_get("answers_json")?;
    Ok(PracticeAttempt {
        id: r.try_get("id")?,
        practice_test_id: r.try_get("practice_test_id")?,
        score_percentage: r.try_get("score_percentage")?,
        answers: serde_json::from_str(&answers_json).unwrap_or(serde_json::Value::Null),
        completed_at: r.try_get("completed_at")?,
    })
}

pub async fn create_test(pool: &SqlitePool, course_id: &str, title: &str, difficulty: PracticeDifficulty) -> Result<PracticeTest, CoreError> {
    let id = new_id();
    sqlx::query("INSERT INTO practice_tests (id, course_id, title, difficulty) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(course_id)
        .bind(title)
        .bind(difficulty.as_str())
        .execute(pool)
        .await?;
    get_test(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub struct NewPracticeQuestion {
    pub order_index: i64,
    pub kind: QuestionKind,
    pub topic: Option<String>,
    pub question_text: String,
    pub options: Option<Vec<String>>,
    pub correct_answer: String,
    pub explanation: Option<String>,
}

pub async fn insert_questions(pool: &SqlitePool, test_id: &str, items: Vec<NewPracticeQuestion>) -> Result<(), CoreError> {
    let mut tx = pool.begin().await?;
    for item in items {
        let options_json = item.options.as_ref().map(|o| serde_json::to_string(o).unwrap_or_default());
        sqlx::query(
            "INSERT INTO practice_questions (id, practice_test_id, order_index, kind, topic, question_text, options_json, correct_answer, explanation) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(test_id)
        .bind(item.order_index)
        .bind(item.kind.as_str())
        .bind(&item.topic)
        .bind(&item.question_text)
        .bind(&options_json)
        .bind(&item.correct_answer)
        .bind(&item.explanation)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn get_test(pool: &SqlitePool, id: &str) -> Result<Option<PracticeTest>, CoreError> {
    let row = sqlx::query(&format!("SELECT {TEST_COLUMNS} FROM practice_tests WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_test).transpose()
}

pub async fn list_by_course(pool: &SqlitePool, course_id: &str) -> Result<Vec<PracticeTest>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {TEST_COLUMNS} FROM practice_tests WHERE course_id = ? ORDER BY created_at DESC"))
        .bind(course_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_test).collect()
}

pub async fn list_questions(pool: &SqlitePool, test_id: &str) -> Result<Vec<PracticeQuestion>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {QUESTION_COLUMNS} FROM practice_questions WHERE practice_test_id = ? ORDER BY order_index"))
        .bind(test_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_question).collect()
}

pub async fn record_attempt(pool: &SqlitePool, test_id: &str, score_percentage: f64, answers: &serde_json::Value) -> Result<PracticeAttempt, CoreError> {
    let id = new_id();
    let answers_json = serde_json::to_string(answers).unwrap_or_else(|_| "null".to_string());
    sqlx::query("INSERT INTO practice_attempts (id, practice_test_id, score_percentage, answers_json) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(test_id)
        .bind(score_percentage)
        .bind(&answers_json)
        .execute(pool)
        .await?;
    let row = sqlx::query(&format!("SELECT {ATTEMPT_COLUMNS} FROM practice_attempts WHERE id = ?"))
        .bind(&id)
        .fetch_one(pool)
        .await?;
    row_to_attempt(row)
}

pub async fn list_attempts(pool: &SqlitePool, test_id: &str) -> Result<Vec<PracticeAttempt>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {ATTEMPT_COLUMNS} FROM practice_attempts WHERE practice_test_id = ? ORDER BY completed_at DESC"))
        .bind(test_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_attempt).collect()
}
