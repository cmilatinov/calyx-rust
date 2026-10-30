use crate::{Segment, SourceFile};
use std::path::{Path, PathBuf};

/// Where each line of a preprocessed shader came from.
///
/// Preprocessing keeps lines whole: an include's text starts on a line of
/// its own, and a parameter's replacement holds no line break. So each line
/// of the expanded source is one line of one file, give or take the columns
/// a replacement shifts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {
    files: Vec<PathBuf>,
    /// For each expanded line, the index of its file and its line there,
    /// from 1.
    lines: Vec<(usize, usize)>,
}

/// A line of one of a shader's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation<'a> {
    /// The file, as the loader resolved it.
    pub file: &'a Path,
    /// The line in `file`, from 1.
    pub line: usize,
}

impl SourceMap {
    /// Maps the lines `segments` join into.
    pub(crate) fn new(files: &[SourceFile], segments: &[Segment]) -> Self {
        let mut lines = Vec::new();
        // The line the next character of the output starts, if it starts
        // one: the first character of a line gives the line its origin.
        let mut pending = Some((0, 1));
        for segment in segments {
            let mut line = segment.first_line;
            for char in segment.text.chars() {
                if pending.take().is_some() {
                    lines.push((segment.file, line));
                }
                if char == '\n' {
                    line += 1;
                    pending = Some((segment.file, line));
                }
            }
        }
        // The last line, when the text ends with a line break or is empty.
        if let Some(origin) = pending {
            lines.push(origin);
        }
        Self {
            files: files.iter().map(|file| file.path.clone()).collect(),
            lines,
        }
    }

    /// Where line `line` of the expanded source, from 1, came from, or
    /// `None` past its last line.
    pub fn location(&self, line: usize) -> Option<SourceLocation<'_>> {
        let &(file, line) = self.lines.get(line.checked_sub(1)?)?;
        Some(SourceLocation {
            file: &self.files[file],
            line,
        })
    }

    /// The number of lines of the expanded source.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the map has no line, which only a default map has.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The files of the shader, in the order they were first included,
    /// the shader itself first.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }
}
