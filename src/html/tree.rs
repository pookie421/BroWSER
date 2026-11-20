use super::{Parser, is_html_space_char};
use std::borrow::Cow;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TreeMode {
    Bootstrap,
    Document(DocumentPhase),
    DivFragment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentPhase {
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    AfterBody,
    AfterHtml,
}

use DocumentPhase::*;
use TreeMode::Document;

impl DocumentPhase {
    fn document_element(self) -> Option<(&'static str, Self)> {
        match self {
            BeforeHtml => Some(("html", BeforeHead)),
            BeforeHead => Some(("head", InHead)),
            AfterHead => Some(("body", InBody)),
            _ => None,
        }
    }
}

impl Parser<'_> {
    pub(super) fn place_start_tag(
        &mut self,
        name: String,
        attributes: BTreeMap<String, String>,
    ) -> Result<(), String> {
        if self.mode == TreeMode::Bootstrap {
            return self.push_element(name, attributes);
        }
        while let Document(phase) = self.mode {
            match phase {
                InHead if metadata(&name) => return self.push_element(name, attributes),
                InHead => self.close_head()?,
                AfterHead if metadata(&name) => {
                    let html = self.stack[1];
                    let head = self.document.nodes[html].children[0];
                    self.stack.push(head);
                    let result = self.push_element(name, attributes);
                    self.stack.pop();
                    return result;
                }
                InBody => break,
                AfterBody | AfterHtml => {
                    return Err(
                        self.error("start tags after the body are outside this document grammar")
                    );
                }
                _ => {
                    let (element, next) = phase.document_element().expect("document start phase");
                    if name == element {
                        return self.open_document_element(name, next, attributes);
                    }
                    self.open_document_element(element.to_owned(), next, BTreeMap::new())?;
                }
            }
        }
        if closes_paragraph(&name) {
            self.close_paragraph();
        } else if !body_element(&name) && !metadata(&name) {
            return Err(self.error(&format!(
                "<{name}> is outside this controlled HTML tree grammar"
            )));
        }
        self.push_element(name, attributes)
    }

    pub(super) fn place_end_tag(&mut self, name: &str) -> Result<(), String> {
        if let Document(phase) = self.mode {
            match name {
                "head" => {
                    let mut phase = phase;
                    while matches!(phase, BeforeHtml | BeforeHead) {
                        let (element, next) =
                            phase.document_element().expect("document start phase");
                        self.open_document_element(element.to_owned(), next, BTreeMap::new())?;
                        phase = next;
                    }
                    if phase == InHead {
                        return self.close_head();
                    }
                }
                "body" | "html" => return self.close_document_body(name),
                _ if phase == InBody => {}
                _ => return Err(self.error(&format!("unexpected closing </{name}> tag"))),
            }
        }
        if self.mode != TreeMode::Bootstrap
            && let Some(paragraph) = self.open_paragraph()
            && (name == "p" || self.element_name(self.stack[paragraph - 1]) == name)
        {
            self.stack.truncate(paragraph);
            if name == "p" {
                return Ok(());
            }
        }
        self.pop_element(name)
    }

    pub(super) fn place_text(&mut self, text: String) -> Result<(), String> {
        let mut content = text.as_str();
        while let Document(phase) = self.mode {
            let trimmed = content.trim_start_matches(is_html_space_char);
            match phase {
                BeforeHtml | BeforeHead => {
                    if trimmed.is_empty() {
                        return Ok(());
                    }
                    content = trimmed;
                }
                InHead | AfterHead => {
                    if trimmed.is_empty() {
                        break;
                    }
                    let whitespace = &content[..content.len() - trimmed.len()];
                    if !whitespace.is_empty() {
                        self.append_text(Cow::Borrowed(whitespace))?;
                    }
                    content = trimmed;
                }
                InBody => break,
                AfterBody | AfterHtml if trimmed.is_empty() => break,
                AfterBody | AfterHtml => {
                    return Err(self
                        .error("non-whitespace after the body is outside this document grammar"));
                }
            }
            if phase == InHead {
                self.close_head()?;
            } else if let Some((element, next)) = phase.document_element() {
                self.open_document_element(element.to_owned(), next, BTreeMap::new())?;
            }
        }
        if content.len() == text.len() {
            self.append_text(Cow::Owned(text))
        } else {
            self.append_text(Cow::Borrowed(content))
        }
    }

    pub(super) fn finish_tree(&mut self) -> Result<(), String> {
        if let Document(_) = self.mode {
            self.imply_document_body()?;
            self.close_paragraph();
            self.require_closed(3)
        } else {
            if self.mode == TreeMode::DivFragment {
                self.close_paragraph();
            }
            self.require_closed(1)
        }
    }

    fn open_document_element(
        &mut self,
        name: String,
        phase: DocumentPhase,
        attributes: BTreeMap<String, String>,
    ) -> Result<(), String> {
        self.push_element(name, attributes)?;
        self.mode = Document(phase);
        Ok(())
    }

    fn close_head(&mut self) -> Result<(), String> {
        self.pop_element("head")?;
        self.mode = Document(AfterHead);
        Ok(())
    }

    fn imply_document_body(&mut self) -> Result<(), String> {
        while let Document(phase) = self.mode {
            if let Some((element, next)) = phase.document_element() {
                self.open_document_element(element.to_owned(), next, BTreeMap::new())?;
            } else if phase == InHead {
                self.close_head()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn close_document_body(&mut self, name: &str) -> Result<(), String> {
        self.imply_document_body()?;
        match self.mode {
            Document(InBody) => {
                self.close_paragraph();
                self.require_closed(3)?;
            }
            Document(AfterBody) if name == "html" => {}
            _ => return Err(self.error(&format!("unexpected closing </{name}> tag"))),
        }
        // Keep the body as the insertion parent for trailing HTML whitespace.
        self.mode = Document(if name == "html" { AfterHtml } else { AfterBody });
        Ok(())
    }

    fn open_paragraph(&self) -> Option<usize> {
        self.stack
            .iter()
            .rposition(|&id| self.element_name(id) == "p")
    }

    fn close_paragraph(&mut self) {
        if let Some(paragraph) = self.open_paragraph() {
            self.stack.truncate(paragraph);
        }
    }

    fn require_closed(&self, depth: usize) -> Result<(), String> {
        if self.stack.len() > depth {
            let name = self.element_name(*self.stack.last().expect("root always exists"));
            return Err(self.error(&format!("unclosed <{name}> element")));
        }
        Ok(())
    }
}

fn metadata(name: &str) -> bool {
    matches!(
        name,
        "base" | "link" | "meta" | "script" | "style" | "title"
    )
}

// The controlled subset avoids additional insertion modes, scope boundaries,
// and the active formatting list.
fn body_element(name: &str) -> bool {
    matches!(
        name,
        "abbr"
            | "bdi"
            | "bdo"
            | "br"
            | "cite"
            | "data"
            | "img"
            | "kbd"
            | "mark"
            | "q"
            | "samp"
            | "span"
            | "time"
            | "var"
            | "wbr"
    )
}

fn closes_paragraph(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "dialog"
            | "div"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "hgroup"
            | "hr"
            | "main"
            | "nav"
            | "p"
            | "search"
            | "section"
            | "summary"
    )
}
