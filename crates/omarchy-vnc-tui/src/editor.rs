use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn edit(text: &mut String, cursor: &mut usize, key: KeyEvent) {
    *cursor = (*cursor).min(text.len());
    let previous = || {
        text[..*cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let next = || {
        text[*cursor..]
            .graphemes(true)
            .next()
            .map(|s| *cursor + s.len())
            .unwrap_or(*cursor)
    };
    match key.code {
        KeyCode::Left => *cursor = previous(),
        KeyCode::Right => *cursor = next(),
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = text.len(),
        KeyCode::Backspace => {
            let p = previous();
            text.replace_range(p..*cursor, "");
            *cursor = p;
        }
        KeyCode::Delete => {
            let n = next();
            text.replace_range(*cursor..n, "");
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            text.clear();
            *cursor = 0;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !c.is_control() =>
        {
            text.insert(*cursor, c);
            *cursor += c.len_utf8();
        }
        _ => {}
    }
}
/// Display a horizontal slice around the cursor, using terminal cell widths and grapheme boundaries.
pub(crate) fn visible(text: &str, cursor: usize, width: usize, masked: bool) -> (String, usize) {
    let chars: Vec<_> = text.grapheme_indices(true).collect();
    let cursor_index = chars.iter().take_while(|(i, _)| *i < cursor).count();
    let cells: Vec<String> = chars
        .iter()
        .map(|(_, s)| {
            if masked {
                "•".into()
            } else {
                s.chars().filter(|c| !c.is_control()).collect()
            }
        })
        .collect();
    let mut start = cursor_index;
    let mut used = 0;
    while start > 0 {
        let w = cells[start - 1].width();
        if used + w >= width.max(1) {
            break;
        }
        used += w;
        start -= 1;
    }
    let mut out = String::new();
    let mut size = 0;
    for cell in &cells[start..] {
        let w = cell.width();
        if size + w > width {
            break;
        }
        out.push_str(cell);
        size += w;
    }
    (out, used.min(width.saturating_sub(1)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_editing() {
        let mut s = "aő👩‍💻".to_string();
        let mut pos = s.len();
        edit(
            &mut s,
            &mut pos,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(s, "aő");
        edit(
            &mut s,
            &mut pos,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        );
        edit(
            &mut s,
            &mut pos,
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
        );
        assert_eq!(s, "a");
    }
    #[test]
    fn masked_horizontal_view() {
        let (s, pos) = visible("secret", 6, 4, true);
        assert_eq!(s, "•••");
        assert_eq!(pos, 3);
        assert!(!s.contains("secret"));
        let (s, pos) = visible("abc界", 6, 4, false);
        assert_eq!(s, "c界");
        assert_eq!(pos, 3);
    }
}
