//! Document-owned DOM nodes in a flat arena.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    Document, MAX_ATTRIBUTES, MAX_DEPTH, MAX_INPUT_BYTES, MAX_NODES, Node, NodeKind, is_html_space,
    is_void,
};

static NEXT_DOCUMENT: AtomicU64 = AtomicU64::new(1);
// An input consisting entirely of text can also produce these implied names.
const MAX_DATA_BYTES: usize = MAX_INPUT_BYTES + "htmlheadbody".len();

/// Stable identity of a node in one document, including while detached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    owner: u64,
    index: usize,
}

impl NodeId {
    /// The document-local numeric ID used by inspection and layout output.
    pub fn index(self) -> usize {
        self.index
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomError {
    WrongDocument,
    UnknownNode,
    InvalidParent,
    InvalidChild,
    NotAChild,
    Cycle,
    DoctypeOrder,
    NodeLimit,
    DepthLimit,
    DataLimit,
    AttributeLimit,
    InvalidElementName,
    InvalidAttributeName,
    ExpectedElement,
    ExpectedText,
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongDocument => f.write_str("node belongs to a different document"),
            Self::UnknownNode => f.write_str("node is outside the document arena"),
            Self::InvalidParent => f.write_str("parent cannot contain this node type"),
            Self::InvalidChild => f.write_str("node cannot be inserted here"),
            Self::NotAChild => f.write_str("node is not a child of this parent"),
            Self::Cycle => f.write_str("insertion would create a cycle"),
            Self::DoctypeOrder => f.write_str("doctype must occur once, before all elements"),
            Self::NodeLimit => write!(f, "node count exceeds {MAX_NODES} limit"),
            Self::DepthLimit => write!(f, "element depth exceeds {MAX_DEPTH} limit"),
            Self::DataLimit => write!(f, "document data exceeds {MAX_DATA_BYTES}-byte limit"),
            Self::AttributeLimit => write!(f, "attribute count exceeds {MAX_ATTRIBUTES} limit"),
            Self::InvalidElementName => f.write_str("expected an ASCII HTML element name"),
            Self::InvalidAttributeName => f.write_str("invalid bootstrap HTML attribute name"),
            Self::ExpectedElement => f.write_str("attribute access requires an element"),
            Self::ExpectedText => f.write_str("text access requires a text node"),
        }
    }
}

impl std::error::Error for DomError {}

fn next_document() -> u64 {
    NEXT_DOCUMENT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("document identity space exhausted")
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Document {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            root: self.root,
            owner: next_document(),
            data_bytes: self.data_bytes,
        }
    }
}

impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.nodes == other.nodes
    }
}

impl Eq for Document {}

impl Document {
    /// Create an empty document with one synthetic root.
    ///
    /// The root can contain multiple elements or text for controlled fragments.
    /// Ownership is separate for each document, including document clones.
    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                kind: NodeKind::Document,
            }],
            root: 0,
            owner: next_document(),
            data_bytes: 0,
        }
    }

    /// Read-only arena view, including detached nodes, indexed by numeric ID.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn root_node(&self) -> NodeId {
        self.handle(self.root)
    }

    /// Bind a numeric inspection ID to this document.
    pub fn node_id(&self, index: usize) -> Option<NodeId> {
        self.nodes.get(index).map(|_| self.handle(index))
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, DomError> {
        Ok(&self.nodes[self.index(id)?])
    }

    pub fn parent(&self, id: NodeId) -> Result<Option<NodeId>, DomError> {
        Ok(self.node(id)?.parent.map(|index| self.handle(index)))
    }

    pub fn children(
        &self,
        id: NodeId,
    ) -> Result<impl ExactSizeIterator<Item = NodeId> + '_, DomError> {
        Ok(self
            .node(id)?
            .children
            .iter()
            .map(|&index| self.handle(index)))
    }

    /// Create a detached HTML element. Names are ASCII case-insensitive.
    pub fn create_element(&mut self, name: &str) -> Result<NodeId, DomError> {
        let mut bytes = name.bytes();
        if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(DomError::InvalidElementName);
        }
        self.allocate(name.len(), || NodeKind::Element {
            name: name.to_ascii_lowercase(),
            attributes: BTreeMap::new(),
        })
    }

    /// Create a detached UTF-8 text node.
    pub fn create_text(&mut self, text: &str) -> Result<NodeId, DomError> {
        self.allocate(text.len(), || NodeKind::Text(text.to_owned()))
    }

    /// Append a node, moving it from its old parent if it is already attached.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        self.insert_child(parent, child, None)
    }

    /// Insert a node immediately before an existing child of the parent.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: NodeId,
    ) -> Result<(), DomError> {
        self.insert_child(parent, child, Some(reference))
    }

    /// Detach a child without destroying its identity or descendants.
    ///
    /// Detached nodes remain owned and count toward the document's arena limits.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent = self.index(parent)?;
        let child = self.index(child)?;
        let position = child_position(&self.nodes[parent], child)?;
        self.nodes[parent].children.remove(position);
        self.nodes[child].parent = None;
        Ok(())
    }

    pub fn attribute(&self, id: NodeId, name: &str) -> Result<Option<&str>, DomError> {
        let NodeKind::Element { attributes, .. } = &self.node(id)?.kind else {
            return Err(DomError::ExpectedElement);
        };
        Ok(attributes.get(&attribute_name(name)?).map(String::as_str))
    }

    pub fn set_attribute(&mut self, id: NodeId, name: &str, value: &str) -> Result<(), DomError> {
        let index = self.index(id)?;
        let NodeKind::Element { attributes, .. } = &mut self.nodes[index].kind else {
            return Err(DomError::ExpectedElement);
        };
        let name = attribute_name(name)?;
        let old_size = if let Some(old) = attributes.get(&name) {
            name.len() + old.len()
        } else if attributes.len() == MAX_ATTRIBUTES {
            return Err(DomError::AttributeLimit);
        } else {
            0
        };
        let new_size = name
            .len()
            .checked_add(value.len())
            .ok_or(DomError::DataLimit)?;
        let data_bytes = updated_data_bytes(self.data_bytes, old_size, new_size)?;
        attributes.insert(name, value.to_owned());
        self.data_bytes = data_bytes;
        Ok(())
    }

    pub fn remove_attribute(&mut self, id: NodeId, name: &str) -> Result<Option<String>, DomError> {
        let index = self.index(id)?;
        let NodeKind::Element { attributes, .. } = &mut self.nodes[index].kind else {
            return Err(DomError::ExpectedElement);
        };
        let name = attribute_name(name)?;
        let removed = attributes.remove(&name);
        if let Some(value) = &removed {
            self.data_bytes -= name.len() + value.len();
        }
        Ok(removed)
    }

    pub fn text(&self, id: NodeId) -> Result<&str, DomError> {
        match &self.node(id)?.kind {
            NodeKind::Text(text) => Ok(text),
            _ => Err(DomError::ExpectedText),
        }
    }

    pub fn set_text(&mut self, id: NodeId, text: &str) -> Result<(), DomError> {
        let index = self.index(id)?;
        let NodeKind::Text(old) = &mut self.nodes[index].kind else {
            return Err(DomError::ExpectedText);
        };
        let data_bytes = updated_data_bytes(self.data_bytes, old.len(), text.len())?;
        *old = text.to_owned();
        self.data_bytes = data_bytes;
        Ok(())
    }

    pub(super) fn measure_data(&mut self) {
        self.data_bytes = self
            .nodes
            .iter()
            .map(|node| match &node.kind {
                NodeKind::Document => 0,
                NodeKind::Doctype(text) | NodeKind::Text(text) => text.len(),
                NodeKind::Element { name, attributes } => {
                    name.len()
                        + attributes
                            .iter()
                            .map(|(name, value)| name.len() + value.len())
                            .sum::<usize>()
                }
            })
            .sum();
        debug_assert!(self.data_bytes <= MAX_DATA_BYTES);
    }

    /// Validate detached trees without adding them to the renderer's tree order.
    pub(crate) fn validate_detached_nodes(&self, seen: &mut [bool]) -> Result<(), String> {
        let mut pending = Vec::new();
        for (root, node) in self.nodes.iter().enumerate() {
            if seen[root] || node.parent.is_some() {
                continue;
            }
            pending.push((root, usize::from(is_element(node))));
            while let Some((index, depth)) = pending.pop() {
                if std::mem::replace(&mut seen[index], true) {
                    return Err("detached tree contains a cycle or shared child".to_owned());
                }
                let node = &self.nodes[index];
                if depth > MAX_DEPTH
                    || matches!(node.kind, NodeKind::Document)
                    || matches!(node.kind, NodeKind::Doctype(_)) && index != root
                    || !is_element(node) && !node.children.is_empty()
                {
                    return Err(
                        "detached tree contains an invalid node or exceeds depth limit".to_owned(),
                    );
                }
                for &child in &node.children {
                    let child_node = self
                        .nodes
                        .get(child)
                        .filter(|node| node.parent == Some(index))
                        .ok_or_else(|| {
                            "detached tree has an inconsistent parent and child".to_owned()
                        })?;
                    pending.push((child, depth + usize::from(is_element(child_node))));
                }
            }
        }
        if seen.iter().any(|&visited| !visited) {
            return Err("document contains unreachable nodes".to_owned());
        }
        Ok(())
    }

    fn handle(&self, index: usize) -> NodeId {
        NodeId {
            owner: self.owner,
            index,
        }
    }

    fn index(&self, id: NodeId) -> Result<usize, DomError> {
        if id.owner != self.owner {
            return Err(DomError::WrongDocument);
        }
        if id.index >= self.nodes.len() {
            return Err(DomError::UnknownNode);
        }
        Ok(id.index)
    }

    fn allocate(
        &mut self,
        bytes: usize,
        make_kind: impl FnOnce() -> NodeKind,
    ) -> Result<NodeId, DomError> {
        let index = self.nodes.len();
        if index == MAX_NODES {
            return Err(DomError::NodeLimit);
        }
        let data_bytes = updated_data_bytes(self.data_bytes, 0, bytes)?;
        self.nodes.push(Node {
            parent: None,
            children: Vec::new(),
            kind: make_kind(),
        });
        self.data_bytes = data_bytes;
        Ok(self.handle(index))
    }

    fn insert_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> Result<(), DomError> {
        let parent = self.index(parent)?;
        let child = self.index(child)?;
        let parent_node = &self.nodes[parent];
        let position = if let Some(reference) = reference {
            let reference = self.index(reference)?;
            let position = child_position(parent_node, reference)?;
            if child == reference {
                return Ok(());
            }
            position
        } else {
            parent_node.children.len()
        };
        let child_kind = &self.nodes[child].kind;
        match &parent_node.kind {
            NodeKind::Document => {}
            NodeKind::Element { name, .. }
                if !is_void(name)
                    && (!matches!(name.as_str(), "script" | "style" | "title")
                        || matches!(child_kind, NodeKind::Text(_))) => {}
            _ => return Err(DomError::InvalidParent),
        }
        if child == self.root || matches!(child_kind, NodeKind::Doctype(_)) && parent != self.root {
            return Err(DomError::InvalidChild);
        }
        if parent == self.root {
            self.check_doctype_position(child, position)?;
        }
        let mut cursor = Some(parent);
        let mut parent_depth = 0;
        while let Some(ancestor) = cursor {
            if ancestor == child {
                return Err(DomError::Cycle);
            }
            let node = &self.nodes[ancestor];
            parent_depth += usize::from(is_element(node));
            cursor = node.parent;
        }
        let mut pending = vec![(child, parent_depth)];
        while let Some((index, depth)) = pending.pop() {
            let node = &self.nodes[index];
            let depth = depth + usize::from(is_element(node));
            if depth > MAX_DEPTH {
                return Err(DomError::DepthLimit);
            }
            pending.extend(node.children.iter().map(|&child| (child, depth)));
        }

        let mut position = position;
        if let Some(old_parent) = self.nodes[child].parent {
            let old_position = child_position(&self.nodes[old_parent], child)?;
            self.nodes[old_parent].children.remove(old_position);
            if old_parent == parent && old_position < position {
                position -= 1;
            }
        }
        self.nodes[parent].children.insert(position, child);
        self.nodes[child].parent = Some(parent);
        Ok(())
    }

    fn check_doctype_position(&self, child: usize, position: usize) -> Result<(), DomError> {
        let siblings = &self.nodes[self.root].children;
        match &self.nodes[child].kind {
            NodeKind::Doctype(_) => {
                if siblings.iter().any(|&index| {
                    index != child && matches!(self.nodes[index].kind, NodeKind::Doctype(_))
                }) || siblings[..position]
                    .iter()
                    .any(|&index| is_element(&self.nodes[index]))
                {
                    return Err(DomError::DoctypeOrder);
                }
            }
            NodeKind::Element { .. }
                if siblings[position..]
                    .iter()
                    .any(|&index| matches!(self.nodes[index].kind, NodeKind::Doctype(_))) =>
            {
                return Err(DomError::DoctypeOrder);
            }
            _ => {}
        }
        Ok(())
    }
}

fn child_position(parent: &Node, child: usize) -> Result<usize, DomError> {
    parent
        .children
        .iter()
        .position(|&index| index == child)
        .ok_or(DomError::NotAChild)
}

fn is_element(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Element { .. })
}

fn attribute_name(name: &str) -> Result<String, DomError> {
    if name.len() > MAX_DATA_BYTES {
        return Err(DomError::DataLimit);
    }
    if name.is_empty()
        || name.bytes().any(|byte| {
            !byte.is_ascii()
                || is_html_space(byte)
                || matches!(byte, 0 | b'=' | b'>' | b'/' | b'\'' | b'"' | b'<' | b'`')
        })
    {
        return Err(DomError::InvalidAttributeName);
    }
    Ok(name.to_ascii_lowercase())
}

fn updated_data_bytes(total: usize, old: usize, new: usize) -> Result<usize, DomError> {
    let retained = total - old;
    if new > MAX_DATA_BYTES - retained {
        return Err(DomError::DataLimit);
    }
    Ok(retained + new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_limit_counts_all_owned_payload_and_reclaims_replaced_strings() {
        let mut doc = Document::new();
        let div = doc.create_element("div").unwrap();
        let text = doc.create_text(&"x".repeat(MAX_DATA_BYTES - 4)).unwrap();
        assert_eq!(doc.create_text("12"), Err(DomError::DataLimit));
        doc.set_attribute(div, "a", "").unwrap();
        assert_eq!(doc.set_attribute(div, "a", "x"), Err(DomError::DataLimit));
        assert_eq!(doc.attribute(div, "a"), Ok(Some("")));
        assert_eq!(
            doc.set_text(text, &"y".repeat(MAX_DATA_BYTES - 3)),
            Err(DomError::DataLimit)
        );
        assert_eq!(doc.text(text).unwrap().len(), MAX_DATA_BYTES - 4);
        assert_eq!(doc.remove_attribute(div, "a"), Ok(Some(String::new())));
        doc.set_text(text, "").unwrap();
        doc.set_attribute(div, "a", &"z".repeat(MAX_DATA_BYTES - 4))
            .unwrap();
        assert_eq!(doc.create_text("over the cap"), Err(DomError::DataLimit));
    }

    #[test]
    fn detached_validation_rejects_unrooted_cycles() {
        let mut doc = Document::new();
        let orphan = doc.create_element("div").unwrap().index();
        doc.nodes[orphan].parent = Some(orphan);
        doc.nodes[orphan].children.push(orphan);
        assert!(doc.validate_detached_nodes(&mut [true, false]).is_err());
    }
}
