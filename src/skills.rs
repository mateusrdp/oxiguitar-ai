// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Skills (mateusrdp/oxiguitar-ai#10): notes of the user's own that the
//! model reads when they fit, such as composition preferences, habits and
//! how to write a certain style.
//!
//! A skill is a Markdown file in the `skills` folder beside OxiGuitar's
//! `plugins` folder, either `NAME.md` or `NAME/SKILL.md` (the layout other
//! assistants use, so a folder of skills can be shared). It may begin with
//! a header between lines of `---`:
//!
//! ```text
//! ---
//! name: blues-shuffle
//! description: How I write a 12-bar blues shuffle.
//! always: false
//! ---
//! The text the model reads.
//! ```
//!
//! Only each skill's name and description go to the model at the start; it
//! loads the text with its `use_skill` tool when the request fits, so a
//! small model's context isn't spent on notes it doesn't need. A skill
//! with `always: true` is put in the instructions whole instead, for the
//! habits that hold for every request.
//!
//! Nothing here reaches the network or writes a file: skills are only read,
//! each time the user sends a message, so a change to one takes effect at
//! once.

use std::fs;
use std::path::{Path, PathBuf};

/// The most a skill's file may hold; a bigger one is left out and said.
pub const MAX_BYTES: u64 = 64 * 1024;
/// The most skills used at once.
pub const MAX_SKILLS: usize = 64;

/// One skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    /// The name the model asks for it by.
    pub name: String,
    /// When it applies, in a line.
    pub description: String,
    /// Whether it is in the instructions whole, not loaded on request.
    pub always: bool,
    /// What the model reads.
    pub text: String,
}

/// The skills found, and what was left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    /// The skills, by name.
    pub skills: Vec<Skill>,
    /// Sentences about files left out, for the user.
    pub problems: Vec<String>,
}

/// Reads one skill from `text`; `fallback` names it when the header does
/// not (the file's or the folder's name).
pub fn parse(text: &str, fallback: &str) -> Result<Skill, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (header, body) = split_header(text);
    let mut name = String::new();
    let mut description = String::new();
    let mut always = false;
    for line in header.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim().to_lowercase().as_str() {
            "name" => name = value.to_owned(),
            "description" => description = value.to_owned(),
            "always" => always = matches!(value.to_lowercase().as_str(), "true" | "yes" | "on"),
            _ => {}
        }
    }
    if name.trim().is_empty() {
        name = fallback.to_owned();
    }
    let name = one_line(&name);
    if name.is_empty() {
        return Err("it has no name".to_owned());
    }
    let text = body.trim().to_owned();
    if text.is_empty() {
        return Err("it has no text".to_owned());
    }
    if description.trim().is_empty() {
        description = text
            .lines()
            .map(|l| l.trim().trim_start_matches('#').trim())
            .find(|l| !l.is_empty())
            .unwrap_or_default()
            .to_owned();
    }
    let description: String = one_line(&description).chars().take(200).collect();
    Ok(Skill {
        name,
        description,
        always,
        text,
    })
}

/// The header between the first two `---` lines, and what follows; no
/// header when the text doesn't begin with one that closes.
fn split_header(text: &str) -> (&str, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return ("", text);
    };
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (&rest[..at], &rest[at + line.len()..]);
        }
        at += line.len();
    }
    ("", text)
}

fn unquote(value: &str) -> &str {
    let b = value.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Whitespace and line breaks as single spaces.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The skills in `folders`, the user's first: of two with one name, the
/// first found is kept.
pub fn load(folders: &[PathBuf]) -> Loaded {
    let mut out = Loaded::default();
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            if stem.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                let file = path.join("SKILL.md");
                if file.is_file() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    found.push((name, file));
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                found.push((stem, path));
            }
        }
        found.sort();
        for (fallback, path) in found {
            match read(&path, &fallback) {
                Ok(skill) => {
                    let taken = out
                        .skills
                        .iter()
                        .any(|s| s.name.eq_ignore_ascii_case(&skill.name));
                    if taken {
                        continue;
                    }
                    if out.skills.len() >= MAX_SKILLS {
                        out.problems.push(format!(
                            "Skills: only the first {MAX_SKILLS} are used; {} is left out",
                            skill.name
                        ));
                        continue;
                    }
                    out.skills.push(skill);
                }
                Err(e) => out
                    .problems
                    .push(format!("Skill {} is left out: {e}", shown(&path))),
            }
        }
    }
    out
}

fn shown(path: &Path) -> String {
    match path.file_name().and_then(|n| n.to_str()) {
        Some("SKILL.md") => path
            .parent()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            .map_or_else(|| path.display().to_string(), |n| format!("{n}/SKILL.md")),
        Some(name) => name.to_owned(),
        None => path.display().to_string(),
    }
}

fn read(path: &Path, fallback: &str) -> Result<Skill, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_BYTES {
        return Err(format!(
            "it holds {} KiB and a skill may hold {}",
            size.div_ceil(1024),
            MAX_BYTES / 1024
        ));
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| "it isn't UTF-8 text".to_owned())?;
    parse(&text, fallback)
}

/// What goes after the plugin's instructions: the skills in force, and the
/// list of those the model may load. Empty with no skills.
pub fn prompt(skills: &[Skill]) -> String {
    let mut out = String::new();
    let always: Vec<&Skill> = skills.iter().filter(|s| s.always).collect();
    if !always.is_empty() {
        out.push_str("\n\nThe user's skills that always apply:");
        for s in always {
            out.push_str(&format!("\n\n[{}]\n{}", s.name, s.text));
        }
    }
    let ask: Vec<&Skill> = skills.iter().filter(|s| !s.always).collect();
    if !ask.is_empty() {
        out.push_str(
            "\n\nThe user keeps notes of their own, called skills. When a request fits one, call use_skill with its name and follow what it says, before you write. Their names and what they are for:",
        );
        for s in ask {
            out.push_str(&format!("\n- {}: {}", s.name, s.description));
        }
    }
    out
}

/// The skill the model asked for, by name in any case.
pub fn find<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    let name = one_line(name);
    skills
        .iter()
        .find(|s| !s.always && s.name.eq_ignore_ascii_case(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oxiguitar-ai-skills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_header_names_and_describes_a_skill() {
        let s = parse(
            "---\nname: \"blues\"\ndescription: 12-bar shuffle\nalways: yes\n---\n\nUse q. e swing.\n",
            "file",
        )
        .unwrap();
        assert_eq!(s.name, "blues");
        assert_eq!(s.description, "12-bar shuffle");
        assert!(s.always);
        assert_eq!(s.text, "Use q. e swing.");
    }

    #[test]
    fn without_a_header_the_file_names_it_and_the_first_line_describes_it() {
        let s = parse("# Never use the top string\nIt is thin.", "my-habits").unwrap();
        assert_eq!(s.name, "my-habits");
        assert_eq!(s.description, "Never use the top string");
        assert!(!s.always);
        assert!(parse("---\nname: x\n---\n  \n", "x").is_err());
        // An unclosed header is just text.
        assert_eq!(parse("---\nname: x\n", "f").unwrap().name, "f");
    }

    #[test]
    fn files_and_folders_load_and_the_first_name_wins() {
        let user = folder("user");
        let system = folder("system");
        fs::write(user.join("blues.md"), "My blues.").unwrap();
        fs::create_dir(user.join("metal")).unwrap();
        fs::write(
            user.join("metal/SKILL.md"),
            "---\ndescription: Chugs\n---\nPalm mute.",
        )
        .unwrap();
        fs::write(user.join("notes.txt"), "not a skill").unwrap();
        fs::write(user.join(".hidden.md"), "no").unwrap();
        fs::write(system.join("BLUES.md"), "Theirs.").unwrap();
        fs::write(system.join("empty.md"), "").unwrap();
        fs::write(system.join("big.md"), vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        fs::write(system.join("binary.md"), [0xff, 0xfe, 0x00]).unwrap();
        let loaded = load(&[
            user.clone(),
            system.clone(),
            PathBuf::from("/no/such/folder"),
        ]);
        let names: Vec<&str> = loaded.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["blues", "metal"]);
        assert_eq!(loaded.skills[0].text, "My blues.");
        assert_eq!(loaded.skills[1].description, "Chugs");
        assert_eq!(loaded.problems.len(), 3, "{:?}", loaded.problems);
        assert!(
            loaded
                .problems
                .iter()
                .any(|p| p.contains("big.md") && p.contains("65 KiB"))
        );
        let _ = fs::remove_dir_all(user);
        let _ = fs::remove_dir_all(system);
    }

    #[test]
    fn the_prompt_lists_what_is_on_request_and_holds_what_always_applies() {
        assert_eq!(prompt(&[]), "");
        let skills = [
            parse("---\nalways: true\n---\nAnswer short.", "habits").unwrap(),
            parse("---\ndescription: for blues\n---\nSwing.", "blues").unwrap(),
        ];
        let p = prompt(&skills);
        assert!(p.contains("[habits]\nAnswer short."));
        assert!(p.contains("- blues: for blues"));
        assert!(!p.contains("Swing."));
        assert!(find(&skills, " BLUES ").is_some());
        assert!(
            find(&skills, "habits").is_none(),
            "an always skill is already read"
        );
    }
}
