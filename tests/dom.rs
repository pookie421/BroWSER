use browser_engine::html::{
    Document, DomError, MAX_ATTRIBUTES, MAX_DEPTH, MAX_NODES, NodeKind, parse,
};
use browser_engine::render::render;

#[test]
fn append_move_remove_and_reattach_keep_document_local_identity() {
    let mut doc = Document::new();
    let root = doc.root_node();
    let left = doc.create_element("SECTION").unwrap();
    let right = doc.create_element("DIV").unwrap();
    let child = doc.create_element("p").unwrap();
    let text = doc.create_text("A & B").unwrap();
    assert!(
        matches!(&doc.node(left).unwrap().kind, NodeKind::Element { name, .. } if name == "section")
    );
    assert_eq!(doc.parent(child), Ok(None));

    doc.append_child(root, left).unwrap();
    doc.append_child(root, right).unwrap();
    doc.append_child(left, child).unwrap();
    doc.append_child(child, text).unwrap();
    doc.append_child(right, child).unwrap();
    assert_eq!(doc.parent(child), Ok(Some(right)));
    assert_eq!(doc.children(left).unwrap().len(), 0);
    assert_eq!(doc.children(child).unwrap().collect::<Vec<_>>(), [text]);
    assert_eq!(doc.text(text), Ok("A & B"));
    assert_eq!(doc.node_id(child.index()), Some(child));

    doc.remove_child(right, child).unwrap();
    assert_eq!(doc.parent(child), Ok(None));
    assert_eq!(doc.parent(text), Ok(Some(child)));
    assert_eq!(doc.nodes().len(), 5);
    assert!(!doc.canonical_json().contains("\"name\":\"p\""));
    assert!(!doc.canonical_json().contains("A & B"));

    doc.append_child(left, child).unwrap();
    assert_eq!(doc.parent(child), Ok(Some(left)));
    assert!(doc.canonical_json().contains(&format!(
        "\"id\":{},\"parent\":{}",
        child.index(),
        left.index()
    )));
    assert_eq!(doc.text(text), Ok("A & B"));
}

#[test]
fn insertion_order_and_relationship_failures_are_transactional() {
    let mut doc =
        parse("<!doctype html><div><p></p></div><aside></aside><img><style></style>").unwrap();
    let root = doc.root_node();
    let ids = doc.children(root).unwrap().collect::<Vec<_>>();
    let [doctype, div, aside, image, style] = ids[..] else {
        panic!("fixture tree changed")
    };
    let p = doc.children(div).unwrap().next().unwrap();

    let copy = doc.clone();
    let foreign = copy.node_id(p.index()).unwrap();
    assert_eq!(doc, copy);
    assert_eq!(doc.node(foreign), Err(DomError::WrongDocument));
    assert_eq!(copy.node(p), Err(DomError::WrongDocument));
    drop(copy);

    let before = doc.canonical_json();
    assert_eq!(
        doc.append_child(aside, foreign),
        Err(DomError::WrongDocument)
    );
    assert_eq!(doc.append_child(p, div), Err(DomError::Cycle));
    assert_eq!(doc.append_child(div, div), Err(DomError::Cycle));
    assert_eq!(doc.append_child(div, root), Err(DomError::InvalidChild));
    assert_eq!(doc.append_child(image, p), Err(DomError::InvalidParent));
    assert_eq!(doc.append_child(style, p), Err(DomError::InvalidParent));
    assert_eq!(doc.append_child(div, doctype), Err(DomError::InvalidChild));
    assert_eq!(doc.append_child(root, doctype), Err(DomError::DoctypeOrder));
    assert_eq!(
        doc.insert_before(root, div, doctype),
        Err(DomError::DoctypeOrder)
    );
    assert_eq!(doc.insert_before(aside, div, p), Err(DomError::NotAChild));
    assert_eq!(doc.remove_child(aside, p), Err(DomError::NotAChild));
    assert_eq!(doc.canonical_json(), before);
    assert_eq!(doc.parent(p), Ok(Some(div)));

    doc.insert_before(root, aside, div).unwrap();
    doc.insert_before(root, div, div).unwrap();
    assert_eq!(
        doc.children(root).unwrap().collect::<Vec<_>>(),
        [doctype, aside, div, image, style]
    );
    doc.insert_before(root, aside, image).unwrap();
    assert_eq!(
        doc.children(root).unwrap().collect::<Vec<_>>(),
        [doctype, div, aside, image, style]
    );
}

#[test]
fn attribute_and_text_mutations_enforce_type_and_attribute_limits() {
    let mut doc = parse("<p id='initial'>café</p>").unwrap();
    let root = doc.root_node();
    let p = doc.children(root).unwrap().next().unwrap();
    let text = doc.children(p).unwrap().next().unwrap();
    assert_eq!(doc.attribute(p, "ID"), Ok(Some("initial")));
    doc.set_attribute(p, "Id", "new").unwrap();
    assert_eq!(doc.attribute(p, "id"), Ok(Some("new")));
    assert_eq!(doc.remove_attribute(p, "ID"), Ok(Some("new".to_owned())));
    assert_eq!(doc.remove_attribute(p, "id"), Ok(None));
    assert_eq!(
        doc.set_attribute(p, "data name", "x"),
        Err(DomError::InvalidAttributeName)
    );
    assert_eq!(
        doc.set_attribute(text, "id", "x"),
        Err(DomError::ExpectedElement)
    );
    assert_eq!(doc.set_text(p, "invalid"), Err(DomError::ExpectedText));
    doc.set_text(text, "नमस्ते").unwrap();
    assert_eq!(doc.text(text), Ok("नमस्ते"));

    for index in 0..MAX_ATTRIBUTES {
        doc.set_attribute(p, &format!("a-{index}"), "").unwrap();
    }
    assert_eq!(
        doc.set_attribute(p, "new", "x"),
        Err(DomError::AttributeLimit)
    );
    doc.set_attribute(p, "A-0", "updated").unwrap();
    assert_eq!(doc.attribute(p, "a-0"), Ok(Some("updated")));
    assert_eq!(doc.attribute(p, "new"), Ok(None));
}

#[test]
fn subtree_depth_is_checked_before_reparenting() {
    let input = format!(
        "<aside><p><span></span></p></aside>{}{}",
        "<div>".repeat(MAX_DEPTH - 2),
        "</div>".repeat(MAX_DEPTH - 2)
    );
    let mut doc = parse(&input).unwrap();
    let root = doc.root_node();
    let aside = doc.children(root).unwrap().next().unwrap();
    let p = doc.children(aside).unwrap().next().unwrap();
    let span = doc.children(p).unwrap().next().unwrap();
    let maximum_parent = doc.node_id(doc.nodes().len() - 1).unwrap();
    doc.append_child(maximum_parent, p).unwrap();
    assert_eq!(doc.parent(span), Ok(Some(p)));
    let before = doc.canonical_json();
    assert_eq!(doc.append_child(span, aside), Err(DomError::DepthLimit));
    assert_eq!(doc.parent(aside), Ok(Some(root)));
    assert_eq!(doc.canonical_json(), before);
    let text = doc.create_text("at maximum depth").unwrap();
    doc.append_child(span, text).unwrap();
    assert_eq!(doc.parent(text), Ok(Some(span)));
}

#[test]
fn detached_nodes_count_toward_limit_and_drop_with_the_arena() {
    let input = format!(
        "{}{}",
        "<section>".repeat(MAX_DEPTH),
        "</section>".repeat(MAX_DEPTH)
    );
    let mut doc = parse(&input).unwrap();
    let root = doc.root_node();
    let first = doc.children(root).unwrap().next().unwrap();
    doc.remove_child(root, first).unwrap();
    while doc.nodes().len() < MAX_NODES - 1 {
        doc.create_element("div").unwrap();
    }
    let last = doc.create_element("p").unwrap();
    assert_eq!(last.index(), MAX_NODES - 1);
    assert_eq!(doc.create_text("over the cap"), Err(DomError::NodeLimit));
    assert_eq!(doc.children(root).unwrap().len(), 0);
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || drop(doc))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn rendering_uses_the_connected_tree_after_dom_mutation() {
    let mut doc = parse("<main><style>div{height:1px;background:blue}</style><style>div{height:1px;background:red}</style><div></div></main>").unwrap();
    let main = doc.children(doc.root_node()).unwrap().next().unwrap();
    let parts = doc.children(main).unwrap().collect::<Vec<_>>();
    let [blue, red, div] = parts[..] else {
        panic!("fixture tree changed")
    };
    let pixel = |doc: &Document| {
        let outcome = render(doc, 1, 1).unwrap();
        assert!(
            outcome
                .layout_json
                .contains(&format!("\"node\":{},", div.index()))
        );
        outcome
            .ppm
            .strip_prefix(b"P6\n1 1\n255\n")
            .unwrap()
            .to_vec()
    };

    assert_eq!(pixel(&doc), [255, 0, 0]);
    doc.remove_child(main, red).unwrap();
    assert_eq!(pixel(&doc), [0, 0, 255]);
    assert!(!doc.canonical_json().contains("background:red"));
    doc.insert_before(main, red, blue).unwrap();
    assert_eq!(pixel(&doc), [0, 0, 255]);
    doc.insert_before(main, blue, red).unwrap();
    assert_eq!(pixel(&doc), [255, 0, 0]);
    let text = doc.create_text("visible text").unwrap();
    assert_eq!(doc.parent(text), Ok(None));
    assert_eq!(pixel(&doc), [255, 0, 0]);
}
