//! Vertical margin collapsing for the M1 in-flow block formatting subset.

use super::{Display, Document, LayoutBox, Rect};

#[derive(Clone, Copy, Default)]
struct Margin {
    positive: i64,
    negative: i64,
}

impl Margin {
    fn single(value: i64) -> Self {
        Self {
            positive: value.max(0),
            negative: value.min(0),
        }
    }

    fn join(self, other: Self) -> Self {
        Self {
            positive: self.positive.max(other.positive),
            negative: self.negative.min(other.negative),
        }
    }

    fn size(self) -> i64 {
        self.positive + self.negative
    }
}

#[derive(Clone, Copy, Default)]
struct FlowMargins {
    before: Margin,
    after: Margin,
    // Where a collapsed-through child would have started with a bottom border.
    position_before: Margin,
    through: bool,
    absorb_top: bool,
    absorb_bottom: bool,
}

pub(super) fn reflow(doc: &Document, boxes: &mut [LayoutBox], viewport: Rect) {
    let mut indices = vec![None; doc.nodes.len()];
    for (index, item) in boxes.iter().enumerate() {
        indices[item.node] = Some(index);
    }
    let mut margins = vec![FlowMargins::default(); boxes.len()];
    // A box's margin groups depend on its visible children. The layout arena
    // stores their preorder, so a reverse pass visits each child first.
    for (index, item) in boxes.iter().enumerate().rev() {
        margins[index] = compute_margins(doc, boxes, &indices, &margins, item);
    }
    let mut layout = BlockFlow {
        doc,
        boxes,
        indices: &indices,
        margins: &margins,
    };
    let mut cursor = 0;
    for root in children(doc, &indices, doc.root) {
        let flow = margins[root];
        let top = cursor + flow.before.size();
        cursor = top + layout.place(root, top) + flow.after.size();
        layout.clip(root, viewport);
    }
}

fn children<'a>(
    doc: &'a Document,
    indices: &'a [Option<usize>],
    node: usize,
) -> impl DoubleEndedIterator<Item = usize> + Clone + 'a {
    doc.nodes[node]
        .children
        .iter()
        .filter_map(|&child| indices[child])
}

fn compute_margins(
    doc: &Document,
    boxes: &[LayoutBox],
    indices: &[Option<usize>],
    previous: &[FlowMargins],
    item: &LayoutBox,
) -> FlowMargins {
    let style = &item.style;
    let children = children(doc, indices, item.node);
    let nested_block = style.display == Display::Block
        && doc.nodes[item.node].parent != Some(doc.root)
        && !style.clip_children
        && item.image.is_none()
        && item.inline.is_none();
    let top_open = style.padding.top + style.border.top == 0;
    let bottom_open = style.padding.bottom + style.border.bottom == 0;
    let mut flow = FlowMargins {
        before: Margin::single(style.margin.top),
        after: Margin::single(style.margin.bottom),
        ..FlowMargins::default()
    };
    flow.absorb_top = nested_block
        && top_open
        && children
            .clone()
            .next()
            .is_none_or(|child| boxes[child].style.display == Display::Block);
    if flow.absorb_top {
        for child in children.clone() {
            flow.before = flow.before.join(previous[child].before);
            if !previous[child].through {
                break;
            }
        }
    }
    flow.position_before = flow.before;
    flow.absorb_bottom = nested_block
        && bottom_open
        && style.height.is_none()
        && children
            .clone()
            .next_back()
            .is_none_or(|child| boxes[child].style.display == Display::Block);
    if flow.absorb_bottom {
        for child in children.clone().rev() {
            flow.after = flow.after.join(previous[child].after);
            if !previous[child].through {
                break;
            }
        }
    }
    flow.through = nested_block
        && top_open
        && bottom_open
        && style.height.unwrap_or(0) == 0
        && children.clone().all(|child| previous[child].through);
    if flow.through {
        flow.before = flow.before.join(flow.after);
        flow.after = flow.before;
    }
    flow
}

struct BlockFlow<'a> {
    doc: &'a Document,
    boxes: &'a mut [LayoutBox],
    indices: &'a [Option<usize>],
    margins: &'a [FlowMargins],
}

impl BlockFlow<'_> {
    fn place(&mut self, index: usize, top: i64) -> i64 {
        let flow = self.margins[index];
        let item = &mut self.boxes[index];
        let node = item.node;
        let delta = top - item.border_box.y;
        item.border_box.y = top;
        item.content_box.y += delta;
        let content_start = item.content_box.y;
        if let Some((inline, _)) = &mut item.inline {
            inline.shift_vertical(delta);
        }
        let fixed_content_height = if item.image.is_some() || item.inline.is_some() {
            Some(item.content_box.height)
        } else {
            item.style.height
        };
        let non_content_height = item.border_box.height - item.content_box.height;
        let mut cursor = content_start;
        let mut outside_top = flow.absorb_top;
        let mut pending = Margin::default();
        for child in children(self.doc, self.indices, node) {
            let child_flow = self.margins[child];
            let position = if outside_top {
                // The first strut belongs outside this box. Empty children whose
                // strut joins it start at the parent's border edge.
                if child_flow.through { top } else { cursor }
            } else {
                cursor + pending.join(child_flow.position_before).size()
            };
            let height = self.place(child, position);
            if child_flow.through {
                pending = pending.join(child_flow.before);
            } else {
                cursor = position + height;
                pending = child_flow.after;
                outside_top = false;
            }
        }
        if !outside_top && !flow.absorb_bottom {
            cursor += pending.size();
        }
        let content_height =
            fixed_content_height.unwrap_or_else(|| (cursor - content_start).max(0));
        let border_height = content_height + non_content_height;
        self.boxes[index].content_box.height = content_height;
        self.boxes[index].border_box.height = border_height;
        border_height
    }

    fn clip(&mut self, index: usize, clip: Rect) {
        let item = &mut self.boxes[index];
        let node = item.node;
        item.clip = clip;
        let child_clip = if item.style.clip_children {
            let box_ = item.border_box;
            let border = item.style.border;
            clip.intersect(Rect {
                x: box_.x + border.left,
                y: box_.y + border.top,
                width: box_.width - border.left - border.right,
                height: box_.height - border.top - border.bottom,
            })
        } else {
            clip
        };
        if let Some((_, inline_clip)) = &mut item.inline {
            *inline_clip = child_clip;
        }
        for child in children(self.doc, self.indices, node) {
            self.clip(child, child_clip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PngResources, RenderOutput, render, render_m1};
    use super::*;
    use crate::html::{NodeKind, parse_document};

    fn box_at(doc: &Document, out: &RenderOutput, id: &str, y: i64, height: i64) {
        let node = doc.nodes.iter().position(|node| {
            matches!(&node.kind, NodeKind::Element { attributes, .. } if attributes.get("id").is_some_and(|value| value == id))
        }).unwrap();
        let prefix = format!("{{\"node\":{node},");
        let row = out.layout_json.split_once(&prefix).unwrap().1;
        assert!(row.starts_with(&format!("\"tag\":\"div\",\"border_box\":{{\"x\":0,\"y\":{y},\"width\":64,\"height\":{height}}}")), "{id}: {row}");
    }

    #[test]
    fn collapses_adjacent_signed_margins_through_empty_blocks() {
        let doc = parse_document(
            "<style>body{margin:0}#a{height:4px;margin:3px 0 10px}
             #empty{height:0;margin:-6px 0 15px}
             #b{height:4px;margin:12px 0 6px}#c{height:4px;margin-top:-9px}</style>
             <div id=a></div><div style='display:none'></div><div id=empty></div><div id=b></div><div id=c></div>",
        ).unwrap();
        let out = render_m1(&doc, 64, 64, &PngResources::default()).unwrap();
        box_at(&doc, &out, "a", 3, 4);
        box_at(&doc, &out, "empty", 11, 0);
        box_at(&doc, &out, "b", 16, 4);
        box_at(&doc, &out, "c", 17, 4);
        let bootstrap = render(&doc, 64, 64).unwrap();
        box_at(&doc, &bootstrap, "b", 38, 4);
    }

    #[test]
    fn collapses_parent_edges_until_padding_or_overflow_separates_them() {
        for (extra, parent_y, child_y, parent_height, next_y) in [
            ("", 12, 12, 5, 26),
            ("padding:1px 0 2px", 4, 17, 29, 41),
            ("overflow:hidden", 4, 16, 26, 38),
        ] {
            let html = format!(
                "<style>body{{margin:0}}#parent{{margin:4px 0 8px;{extra}}}
                 #child{{margin:12px 0 9px;height:5px}}#next{{margin-top:2px;height:3px}}</style>
                 <div id=parent><div id=child></div></div><div id=next></div>"
            );
            let doc = parse_document(&html).unwrap();
            let out = render_m1(&doc, 64, 64, &PngResources::default()).unwrap();
            box_at(&doc, &out, "parent", parent_y, parent_height);
            box_at(&doc, &out, "child", child_y, 5);
            box_at(&doc, &out, "next", next_y, 3);
        }
    }

    #[test]
    fn collapses_through_nested_zero_height_blocks() {
        let doc = parse_document(
            "<style>body{margin:0;overflow:hidden}#parent{height:0;margin:3px 0 7px}
             #empty{margin:-6px 0 9px}#next{height:1px;margin-top:4px}</style>
             <div id=parent><div id=empty></div></div><div id=next></div>",
        )
        .unwrap();
        let out = render_m1(&doc, 64, 64, &PngResources::default()).unwrap();
        box_at(&doc, &out, "parent", 3, 0);
        box_at(&doc, &out, "empty", 3, 0);
        box_at(&doc, &out, "next", 3, 1);
    }

    #[test]
    fn relocates_text_and_its_overflow_clip_to_the_collapsed_position() {
        let doc = parse_document(
            "<style>body{margin:0}#first{height:4px;margin-bottom:10px}
             #text{height:8px;margin-top:6px;overflow:hidden;line-height:20px}</style>
             <div id=first></div><div id=text>A</div>",
        )
        .unwrap();
        let out = render_m1(&doc, 64, 64, &PngResources::default()).unwrap();
        box_at(&doc, &out, "text", 14, 8);
        let pixels = out.ppm.strip_prefix(b"P6\n64 64\n255\n").unwrap();
        let drawn_rows: Vec<_> = pixels
            .as_chunks::<{ 64 * 3 }>()
            .0
            .iter()
            .enumerate()
            .filter_map(|(row, pixels)| pixels.iter().any(|&value| value < 255).then_some(row))
            .collect();
        assert!(!drawn_rows.is_empty());
        assert!(drawn_rows.into_iter().all(|row| (14..22).contains(&row)));
    }
}
