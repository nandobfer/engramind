//! Catálogo estático dos guias servidos pela documentação viva (portado do sap-middleman).
//!
//! Os guias são compilados no binário a partir de `docs/guides`, então REST, MCP e CLI sempre
//! servem a documentação da versão em execução.

/// Quem pode ler um guia e as operações OpenAPI marcadas com as tags dele.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    All,
    Admin,
}

/// Tags OpenAPI restritas a tokens `admin` (operações, esquemas e tags ocultos para `agent`).
pub const ADMIN_TAGS: &[&str] = &["Identities", "Settings", "Status", "Audit"];

/// Tags sem guia próprio além da visão geral.
pub const OPERATIONAL_TAGS: &[&str] = &["Health", "Operations"];

struct GuideSource {
    id: &'static str,
    file: &'static str,
    audience: Audience,
    tags: &'static [&'static str],
    content: &'static str,
}

const GUIDES: &[GuideSource] = &[
    GuideSource {
        id: "overview",
        file: "overview.md",
        audience: Audience::All,
        tags: &["Health", "Operations", "Documentation"],
        content: include_str!("../../../docs/guides/overview.md"),
    },
    GuideSource {
        id: "tokens-e-escopos",
        file: "tokens-e-escopos.md",
        audience: Audience::All,
        tags: &["Authentication", "Identities"],
        content: include_str!("../../../docs/guides/tokens-e-escopos.md"),
    },
    GuideSource {
        id: "integracao-mcp",
        file: "integracao-mcp.md",
        audience: Audience::All,
        tags: &["MCP"],
        content: include_str!("../../../docs/guides/integracao-mcp.md"),
    },
    GuideSource {
        id: "cli",
        file: "cli.md",
        audience: Audience::All,
        tags: &[],
        content: include_str!("../../../docs/guides/cli.md"),
    },
    GuideSource {
        id: "erros",
        file: "erros.md",
        audience: Audience::All,
        tags: &[],
        content: include_str!("../../../docs/guides/erros.md"),
    },
    GuideSource {
        id: "configuracoes",
        file: "configuracoes.md",
        audience: Audience::Admin,
        tags: &["Settings"],
        content: include_str!("../../../docs/guides/configuracoes.md"),
    },
    GuideSource {
        id: "operacao",
        file: "operacao.md",
        audience: Audience::Admin,
        tags: &["Status", "Audit"],
        content: include_str!("../../../docs/guides/operacao.md"),
    },
];

/// A Markdown heading and the byte range of the content it introduces.
#[derive(Clone, Debug)]
pub struct Section {
    pub heading: String,
    pub level: usize,
    /// Start of the heading line.
    pub start: usize,
    /// End of the section including nested subsections.
    pub end: usize,
    /// End of the section body before the next heading of any level.
    pub body_end: usize,
}

#[derive(Clone, Debug)]
pub struct Guide {
    pub id: &'static str,
    pub file: &'static str,
    pub title: String,
    pub audience: Audience,
    pub tags: &'static [&'static str],
    pub content: &'static str,
    pub sections: Vec<Section>,
}

impl Guide {
    pub fn visible_to(&self, admin: bool) -> bool {
        admin || self.audience == Audience::All
    }

    /// Finds a section by heading, ignoring case, accents and surrounding spaces.
    pub fn section(&self, heading: &str) -> Option<&Section> {
        let wanted = normalize(heading);
        self.sections
            .iter()
            .find(|section| normalize(&section.heading) == wanted)
    }

    pub fn section_text(&self, section: &Section) -> &'static str {
        &self.content[section.start..section.end]
    }

    pub fn section_body(&self, section: &Section) -> &'static str {
        &self.content[section.start..section.body_end]
    }
}

pub fn load_guides() -> Vec<Guide> {
    GUIDES
        .iter()
        .map(|source| {
            let sections = parse_sections(source.content);
            let title = sections
                .iter()
                .find(|section| section.level == 1)
                .map_or_else(|| source.id.to_owned(), |section| section.heading.clone());
            Guide {
                id: source.id,
                file: source.file,
                title,
                audience: source.audience,
                tags: source.tags,
                content: source.content,
                sections,
            }
        })
        .collect()
}

/// Parses ATX headings outside fenced code blocks.
pub fn parse_sections(content: &str) -> Vec<Section> {
    let mut headings: Vec<(usize, usize, String)> = Vec::new();
    let mut in_fence = false;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence {
            let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
            if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
                let heading = trimmed[level..].trim().to_owned();
                if !heading.is_empty() {
                    headings.push((offset, level, heading));
                }
            }
        }
        offset += line.len();
    }

    headings
        .iter()
        .enumerate()
        .map(|(index, (start, level, heading))| {
            let following = &headings[index + 1..];
            let end = following
                .iter()
                .find(|(_, next_level, _)| next_level <= level)
                .map_or(content.len(), |(next_start, _, _)| *next_start);
            let body_end = following
                .first()
                .map_or(content.len(), |(next_start, _, _)| *next_start);
            Section {
                heading: heading.clone(),
                level: *level,
                start: *start,
                end,
                body_end,
            }
        })
        .collect()
}

/// Lowercases and removes Portuguese diacritics for tolerant matching.
pub fn normalize(value: &str) -> String {
    normalize_chars(value.trim()).collect()
}

/// Maps every character to exactly one normalized character, so character
/// positions in the normalized text match the original text.
pub fn normalize_chars(value: &str) -> impl Iterator<Item = char> + '_ {
    value
        .chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .map(|character| match character {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_consumer_guide_file_is_cataloged() {
        // `CARGO_MANIFEST_DIR` em tempo de execução: nos shards do CI o caminho de compilação
        // não existe.
        let directory = std::env::var("CARGO_MANIFEST_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
            .join("docs/guides");
        let files: BTreeSet<String> = std::fs::read_dir(&directory)
            .expect("read docs/guides")
            .map(|entry| entry.expect("docs/guides entry").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".md"))
            .collect();
        let cataloged: BTreeSet<String> =
            GUIDES.iter().map(|guide| guide.file.to_owned()).collect();
        assert_eq!(files, cataloged, "docs/guides e o catalogo divergiram");
    }

    #[test]
    fn guide_ids_are_unique_and_titled() {
        let guides = load_guides();
        let ids: BTreeSet<_> = guides.iter().map(|guide| guide.id).collect();
        assert_eq!(ids.len(), guides.len());
        for guide in &guides {
            assert!(
                guide.sections.iter().any(|section| section.level == 1),
                "{} has no level-1 title",
                guide.id
            );
        }
    }

    #[test]
    fn parses_nested_sections_and_ignores_code_fences() {
        let content =
            "# Title\nintro\n## A\ntext\n```sh\n# not a heading\n```\n### A1\nmore\n## B\nend\n";
        let sections = parse_sections(content);
        let headings: Vec<_> = sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["Title", "A", "A1", "B"]);
        let a = &sections[1];
        assert_eq!(
            &content[a.start..a.end],
            "## A\ntext\n```sh\n# not a heading\n```\n### A1\nmore\n"
        );
        assert_eq!(
            &content[a.start..a.body_end],
            "## A\ntext\n```sh\n# not a heading\n```\n"
        );
        assert_eq!(&content[sections[3].start..sections[3].end], "## B\nend\n");
    }

    #[test]
    fn finds_sections_ignoring_case_and_accents() {
        let guide = load_guides()
            .into_iter()
            .find(|guide| guide.id == "tokens-e-escopos")
            .expect("tokens-e-escopos guide");
        let section = guide.section("  EMISSÃO ").expect("section");
        assert!(guide.section_text(section).starts_with("## Emissão"));
        assert_eq!(normalize("Ação Ç"), "acao c");
    }

    #[test]
    fn administration_guides_are_admin_only() {
        for id in ["configuracoes", "operacao"] {
            let guide = load_guides()
                .into_iter()
                .find(|guide| guide.id == id)
                .expect("admin guide");
            assert!(!guide.visible_to(false), "{id}");
            assert!(guide.visible_to(true), "{id}");
        }
    }
}
