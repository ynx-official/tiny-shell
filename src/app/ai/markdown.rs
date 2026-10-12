//! Model and remote content must not trigger automatic image or HTML resource loading.
use markdown::{ParseOptions, mdast::Node};

pub(super) fn renderable(source: &str) -> String {
    let Ok(tree) = markdown::to_mdast(source, &ParseOptions::gfm()) else {
        return source.replace('!', "\\!").replace('<', "&lt;");
    };
    let mut replacements = Vec::new();
    blocked(&tree, &mut replacements);
    let mut result = source.to_owned();
    for (start, end, html) in replacements.into_iter().rev() {
        let text = &source[start..end];
        let replacement = if html {
            text.replace('<', "&lt;")
        } else {
            format!("\\{text}")
        };
        result.replace_range(start..end, &replacement);
    }
    result
}

fn blocked(node: &Node, ranges: &mut Vec<(usize, usize, bool)>) {
    if matches!(
        node,
        Node::Image(_) | Node::ImageReference(_) | Node::Html(_)
    ) {
        if let Some(pos) = node.position() {
            ranges.push((
                pos.start.offset,
                pos.end.offset,
                matches!(node, Node::Html(_)),
            ));
        }
    } else if let Some(children) = node.children() {
        for child in children {
            blocked(child, ranges);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_and_html_sources_are_literal_but_code_and_text_links_are_unchanged() {
        let source = "![remote](https://example.invalid/private)\n\n![ref][image]\n\n[image]: file:///private.png\n\n<img src='https://example.invalid/private'>\n\n[docs](https://example.invalid/docs)\n\n```sh\necho '![code](example)'\n```\n\n`![inline](example)`";
        let sanitized = renderable(source);
        let tree = markdown::to_mdast(&sanitized, &ParseOptions::gfm()).unwrap();
        let mut ranges = Vec::new();
        blocked(&tree, &mut ranges);
        assert!(ranges.is_empty());
        assert!(sanitized.contains("[docs](https://example.invalid/docs)"));
        assert!(sanitized.contains("```sh\necho '![code](example)'\n```"));
        assert!(sanitized.contains("`![inline](example)`"));
    }
}
