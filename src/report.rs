//! Plain-text report builder shared by every collector.

use std::fmt::{Display, Write};

const LABEL_WIDTH: usize = 30;

#[derive(Default)]
pub struct Report {
    buf: String,
    indent: usize,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(&mut self, title: &str) {
        let _ = writeln!(self.buf, "{title}\n{}", "=".repeat(title.len()));
    }

    pub fn section(&mut self, title: &str) {
        self.indent = 0;
        let rule = "-".repeat(title.chars().count().min(78));
        let _ = writeln!(self.buf, "\n{title}\n{rule}");
    }

    pub fn subsection(&mut self, title: &str) {
        self.indent = 2;
        let _ = writeln!(self.buf, "  [{title}]");
        self.indent = 4;
    }

    pub fn end_subsection(&mut self) {
        self.indent = 0;
    }

    pub fn kv(&mut self, key: &str, value: impl Display) {
        let pad = " ".repeat(self.indent);
        let _ = writeln!(self.buf, "{pad}{key:<LABEL_WIDTH$} {value}");
    }

    pub fn opt_kv(&mut self, key: &str, value: Option<impl Display>) {
        if let Some(v) = value {
            self.kv(key, v);
        }
    }

    pub fn line(&mut self, text: impl Display) {
        let pad = " ".repeat(self.indent);
        let _ = writeln!(self.buf, "{pad}{text}");
    }

    pub fn finish(self) -> String {
        self.buf
    }
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.2} {} ({n} B)", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formats_units() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(8 * 1024 * 1024 * 1024), "8.00 GiB (8589934592 B)");
    }
}
