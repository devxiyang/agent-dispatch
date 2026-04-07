use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DispatchError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct PendingQuestion {
    pub sequence: String,
    pub question_path: PathBuf,
    pub answer_path: PathBuf,
    pub done_path: PathBuf,
    pub question: String,
}

pub fn list_pending_questions(mailbox_dir: &Path) -> Result<Vec<PendingQuestion>> {
    if !mailbox_dir.exists() {
        return Ok(Vec::new());
    }

    let mut pending = Vec::new();
    for entry in fs::read_dir(mailbox_dir).map_err(|source| DispatchError::Io {
        path: mailbox_dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| DispatchError::Io {
            path: mailbox_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("question") {
            continue;
        }

        let sequence = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        let answer_path = mailbox_dir.join(format!("{sequence}.answer"));
        if answer_path.exists() {
            continue;
        }
        let done_path = mailbox_dir.join(format!("{sequence}.done"));
        let question = fs::read_to_string(&path).map_err(|source| DispatchError::Io {
            path: path.clone(),
            source,
        })?;
        pending.push(PendingQuestion {
            sequence,
            question_path: path,
            answer_path,
            done_path,
            question,
        });
    }

    pending.sort_by(|left, right| left.sequence.cmp(&right.sequence));
    Ok(pending)
}

pub fn write_answer_atomic(answer_path: &Path, answer: &str) -> Result<()> {
    let tmp_path = answer_path.with_extension("answer.tmp");
    fs::write(&tmp_path, answer).map_err(|source| DispatchError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    fs::rename(&tmp_path, answer_path).map_err(|source| DispatchError::Io {
        path: answer_path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{list_pending_questions, write_answer_atomic};

    #[test]
    fn lists_pending_questions_in_sequence_order() {
        let temp = tempfile::tempdir().unwrap();
        let mailbox = temp.path().join("mailbox");
        fs::create_dir_all(&mailbox).unwrap();
        fs::write(mailbox.join("002.question"), "second").unwrap();
        fs::write(mailbox.join("001.question"), "first").unwrap();
        fs::write(mailbox.join("002.answer"), "done").unwrap();

        let pending = list_pending_questions(&mailbox).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, "001");
        assert_eq!(pending[0].question.trim(), "first");
    }

    #[test]
    fn writes_answer_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let answer_path = temp.path().join("001.answer");

        write_answer_atomic(&answer_path, "resolved").unwrap();

        assert_eq!(fs::read_to_string(&answer_path).unwrap(), "resolved");
        assert!(!temp.path().join("001.answer.tmp").exists());
    }
}
