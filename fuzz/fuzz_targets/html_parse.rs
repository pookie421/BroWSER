#![no_main]

use browser_engine::html::{self, Document, NodeKind};
use libfuzzer_sys::{Corpus, fuzz_target};

type ParseHtml = fn(&str) -> Result<Document, String>;

fuzz_target!(|bytes: &[u8]| -> Corpus {
    let Ok(input) = std::str::from_utf8(bytes) else {
        return Corpus::Reject;
    };
    let parsers: [ParseHtml; 3] = [html::parse, html::parse_document, |source| {
        html::parse_fragment(source, "div")
    }];
    for parse in parsers {
        let parsed = parse(input);
        assert_eq!(parsed, parse(input));
        if let Ok(document) = parsed {
            assert_tree(&document);
        }
    }
    Corpus::Keep
});

fn assert_tree(document: &Document) {
    let nodes = document.nodes();
    assert!(nodes.len() <= html::MAX_NODES);
    let root = document.root_node().index();
    assert_eq!(nodes[root].parent, None);
    let mut pending = vec![(root, 0)];
    let mut seen = vec![false; nodes.len()];
    while let Some((index, ancestor_elements)) = pending.pop() {
        assert!(!seen[index], "tree contains a repeated node");
        seen[index] = true;
        let node = &nodes[index];
        let depth = match &node.kind {
            NodeKind::Document => {
                assert_eq!(index, root);
                ancestor_elements
            }
            NodeKind::Doctype(_) => {
                assert_eq!(node.parent, Some(root));
                assert!(node.children.is_empty());
                ancestor_elements
            }
            NodeKind::Element { attributes, .. } => {
                assert!(attributes.len() <= html::MAX_ATTRIBUTES);
                ancestor_elements + 1
            }
            NodeKind::Text(_) => {
                assert!(node.children.is_empty());
                ancestor_elements
            }
        };
        assert!(depth <= html::MAX_DEPTH);
        for &child in &node.children {
            assert_eq!(nodes[child].parent, Some(index));
            pending.push((child, depth));
        }
    }
    assert!(seen.iter().all(|visited| *visited));
}
