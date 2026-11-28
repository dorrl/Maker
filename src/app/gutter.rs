use super::*;

fn line_top(editor: Handle, start: i32) -> Option<i32> {
    let mut point = Point { x: 0, y: i32::MIN };
    send(
        editor,
        EM_POSFROMCHAR,
        &mut point as *mut _ as usize,
        start as isize,
    );
    (point.y != i32::MIN).then_some(point.y)
}

fn visible_lines(
    editor: Handle,
    wrap: bool,
    viewport: i32,
    default_height: i32,
    mut draw: impl FnMut(usize, i32, i32),
) {
    let first = send(editor, EM_GETFIRSTVISIBLELINE, 0, 0).max(0) as usize;
    let count = send(editor, EM_GETLINECOUNT, 0, 0).max(1) as usize;
    let start_at = |line| send(editor, EM_LINEINDEX, line, 0) as i32;
    let mut start = start_at(first);
    if first >= count || start < 0 {
        return;
    }
    let (mut number, paragraph) = if wrap && first > 0 {
        let Some((line, paragraph)) = editor_paragraph(editor, start) else {
            return;
        };
        (line + 1, paragraph)
    } else {
        (first + 1, start)
    };
    let mut numbered = start == paragraph;
    let mut format = Rect::default();
    send(editor, EM_GETRECT, 0, &mut format as *mut _ as isize);
    let limit = viewport.min(format.bottom);
    let mut top = line_top(editor, start).unwrap_or(format.top);
    let mut height = default_height;
    for line in first..count {
        if top >= limit {
            break;
        }
        let next = if line + 1 < count {
            start_at(line + 1)
        } else {
            -1
        };
        let next_top = if next >= 0 {
            line_top(editor, next).filter(|&y| y > top)
        } else {
            None
        };
        if let Some(next_top) = next_top {
            height = next_top - top;
        }
        if numbered && top + height > format.top {
            draw(number, top, top + height);
        }
        if next <= start {
            break;
        }
        numbered = !wrap || editor_line_break_before(editor, next);
        number += usize::from(numbered);
        top = next_top.unwrap_or(top + height);
        start = next;
    }
}

impl App {
    pub(super) fn paint_gutter(&self, group: usize, dc: Handle, area: Rect, paint: Paint) {
        let Some(index) = self.group_document(group) else {
            return;
        };
        let editor = self.documents[index].editor;
        let viewport = bounds(editor).height().min(area.height());
        fill_color(
            dc,
            Rect {
                left: area.right - 1,
                bottom: viewport,
                ..area
            },
            motion::mix(paint.editor_rgb, paint.border_rgb, 0.65),
        );
        let current = self.gutter_caret.get();
        let number_right = area.right - self.px(9) - 2 * paint.gutter_cell_width;
        visible_lines(
            editor,
            self.settings.wrap,
            viewport,
            paint.gutter_height,
            |line, top, bottom| {
                let active = current == Some((editor, line));
                if active {
                    fill_color(
                        dc,
                        Rect {
                            top: top.max(0),
                            bottom: bottom.min(viewport),
                            right: area.right - 1,
                            ..area
                        },
                        motion::mix(paint.editor_rgb, paint.selected_rgb, 0.34),
                    );
                }
                let number_width = (line.ilog10() as i32 + 1) * paint.gutter_cell_width;
                draw_line_number(
                    dc,
                    Rect {
                        top,
                        bottom,
                        right: number_right
                            .max(area.left + number_width)
                            .min(area.right - 1),
                        ..area
                    },
                    line,
                    paint.gutter_font,
                    if active {
                        paint.foreground
                    } else {
                        paint.muted
                    },
                );
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_numbers_survive_scrolling_into_a_continuation() -> io::Result<()> {
        let _ole = Ole::init()?;
        let class = "MakerWrappedLineTest";
        setup(class, window_proc)?;
        let window = main_window(class)?;
        let editor = child(window, EDITOR, "RICHEDIT50W", "", ES_MULTILINE | WS_VSCROLL)?;
        place(editor, 0, 0, 220, 360);
        let text = format!(
            "first\nsecond\nthird\n{}\n\nlast line\n{}",
            "alpha beta gamma ".repeat(14),
            "tail\n".repeat(50),
        );
        set_editor_text(editor, &text);
        send(editor, EM_SETTARGETDEVICE, 0, 0);
        let labels = || {
            let mut result = Vec::new();
            visible_lines(editor, true, bounds(editor).height(), 15, |number, _, _| {
                result.push(number)
            });
            result
        };
        let before = labels();
        assert!(before.starts_with(&[1, 2, 3, 4, 5, 6]), "{before:?}");
        send(editor, EM_LINESCROLL, 0, 3);
        let at_start = labels();
        assert!(at_start.starts_with(&[4, 5, 6]), "{at_start:?}");
        send(editor, EM_LINESCROLL, 0, 1);
        let first = send(editor, EM_GETFIRSTVISIBLELINE, 0, 0);
        assert!(
            first > 0,
            "expected scroll, first visible display row is {first}"
        );
        let after = labels();
        assert!(after.starts_with(&[5, 6, 7]), "{after:?}");
        unsafe { DestroyWindow(window) };
        Ok(())
    }
}
