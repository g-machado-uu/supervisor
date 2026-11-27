//! A small PDF writer for a one-table document, such as the list of posters
//! to mark: a title, a line under it and a table, on landscape A4 pages.
//!
//! It uses the standard Helvetica fonts, which every PDF reader has, so
//! nothing is embedded. Text is written in the Windows-1252 encoding, which
//! covers the accented letters of Western European names; anything else is
//! shown as "?".

use std::fmt::Write as _;

/// Landscape A4, in points.
const PAGE_W: f32 = 842.0;
const PAGE_H: f32 = 595.0;
const MARGIN: f32 = 48.0;
/// Text size of the table: as large as fits, between these.
const MAX_SIZE: f32 = 20.0;
const MIN_SIZE: f32 = 11.0;
const LEADING: f32 = 1.25;

/// A column of the table.
pub struct Column {
    pub heading: String,
    /// Centred, as for short numbers; otherwise to the left.
    pub centred: bool,
}

/// A document of one table.
pub struct Table {
    pub title: String,
    /// Under the title, e.g. who and which year.
    pub subtitle: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    /// Small print at the foot of each page.
    pub footer: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Font {
    Regular,
    Bold,
}

impl Font {
    fn name(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
        }
    }
}

/// Widths of Helvetica and Helvetica-Bold (Adobe's metrics, in 1/1000 em)
/// for the printable ASCII characters, space to tilde.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556,
    556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611,
    611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// The Windows-1252 byte for `c`, or `?`.
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{A0}'..='\u{FF}' => c as u32 as u8,
        '€' => 0x80,
        '‚' => 0x82,
        '„' => 0x84,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        'Š' => 0x8A,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        'š' => 0x9A,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        _ => b'?',
    }
}

/// Width of one character in 1/1000 em. Accented letters take the width
/// of their base letter, near enough; others that of a digit.
fn char_width(c: char, font: Font) -> f32 {
    let table = match font {
        Font::Regular => &HELVETICA,
        Font::Bold => &HELVETICA_BOLD,
    };
    let base = match c {
        ' '..='~' => c,
        'À'..='Å' => 'A',
        'Ç' => 'C',
        'È'..='Ë' => 'E',
        'Ì'..='Ï' => 'I',
        'Ñ' => 'N',
        'Ò'..='Ö' | 'Ø' => 'O',
        'Ù'..='Ü' => 'U',
        'Ý' => 'Y',
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '–' => return 556.0,
        '—' => return 1000.0,
        '‘' | '’' => return 222.0,
        _ => '0',
    };
    f32::from(table[(base as usize) - 32])
}

fn text_width(text: &str, font: Font, size: f32) -> f32 {
    text.chars().map(|c| char_width(c, font)).sum::<f32>() * size / 1000.0
}

/// `text` as a PDF string: Windows-1252, with `(`, `)` and `\` escaped.
fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in text.chars() {
        let b = win_ansi(c);
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// `text` split into lines no wider than `width`, at spaces where it can be;
/// a word too long for a line is broken.
fn wrap(text: &str, font: Font, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if text_width(&candidate, font, size) <= width {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        // A word wider than the column on its own: break it.
        for c in word.chars() {
            let mut longer = line.clone();
            longer.push(c);
            if !line.is_empty() && text_width(&longer, font, size) > width {
                lines.push(std::mem::take(&mut line));
                line.push(c);
            } else {
                line = longer;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The page content being drawn.
struct Canvas {
    ops: Vec<u8>,
}

impl Canvas {
    fn op(&mut self, text: &str) {
        self.ops.extend_from_slice(text.as_bytes());
        self.ops.push(b'\n');
    }

    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: (f32, f32, f32)) {
        self.op(&format!("{:.3} {:.3} {:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f", rgb.0, rgb.1, rgb.2));
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, rgb: (f32, f32, f32)) {
        self.op(&format!(
            "{:.3} {:.3} {:.3} RG {width:.2} w {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S",
            rgb.0, rgb.1, rgb.2
        ));
    }

    /// Text with its baseline starting at (`x`, `y`).
    fn text(&mut self, x: f32, y: f32, text: &str, font: Font, size: f32, rgb: (f32, f32, f32)) {
        let mut op = format!(
            "BT {:.3} {:.3} {:.3} rg /{} {size:.2} Tf {x:.2} {y:.2} Td ",
            rgb.0,
            rgb.1,
            rgb.2,
            font.name()
        )
        .into_bytes();
        op.extend(pdf_string(text));
        op.extend_from_slice(b" Tj ET\n");
        self.ops.extend(op);
    }
}

const INK: (f32, f32, f32) = (0.11, 0.13, 0.17);
const MUTED: (f32, f32, f32) = (0.42, 0.45, 0.50);
const HEAD_FILL: (f32, f32, f32) = (0.87, 0.91, 0.96);
const STRIPE: (f32, f32, f32) = (0.965, 0.97, 0.98);
const RULE: (f32, f32, f32) = (0.70, 0.74, 0.80);

/// Padding inside a cell, for text of `size`.
fn padding(size: f32) -> (f32, f32) {
    (size * 0.6, size * 0.55)
}

/// Widths of the columns for text of `size`: each as wide as its widest
/// text, if they all fit in `room`; otherwise shared out by those widths,
/// with long text wrapped. Also whether they fitted.
fn column_widths(t: &Table, size: f32, room: f32) -> (Vec<f32>, bool) {
    let (pad_x, _) = padding(size);
    let natural: Vec<f32> = (0..t.columns.len())
        .map(|i| {
            let heading = text_width(&t.columns[i].heading, Font::Bold, size);
            let widest = t.rows.iter().map(|r| r.get(i).map_or(0.0, |c| text_width(c, Font::Regular, size))).fold(0.0, f32::max);
            heading.max(widest) + 2.0 * pad_x
        })
        .collect();
    let total: f32 = natural.iter().sum();
    if total <= room {
        // Spread what is left over the columns, so the table spans the page.
        let extra = (room - total) / natural.len() as f32;
        return (natural.iter().map(|w| w + extra).collect(), true);
    }
    (natural.iter().map(|w| w / total * room).collect(), false)
}

/// The PDF file of `t`.
pub fn table(t: &Table) -> Vec<u8> {
    let room = PAGE_W - 2.0 * MARGIN;
    // As large as fits on one line per cell, or the smallest size.
    let mut size = MAX_SIZE;
    while size > MIN_SIZE && !column_widths(t, size, room).1 {
        size -= 0.5;
    }
    let (widths, _) = column_widths(t, size, room);
    let (pad_x, mut pad_y) = padding(size);
    let leading = size * LEADING;
    let cell_lines = |text: &str, font: Font, i: usize| wrap(text, font, size, widths[i] - 2.0 * pad_x);

    let header: Vec<Vec<String>> = t.columns.iter().enumerate().map(|(i, c)| cell_lines(&c.heading, Font::Bold, i)).collect();
    let rows: Vec<Vec<Vec<String>>> = t
        .rows
        .iter()
        .map(|r| (0..t.columns.len()).map(|i| cell_lines(r.get(i).map_or("", String::as_str), Font::Regular, i)).collect())
        .collect();

    let title_size = 24.0;
    let sub_size = 12.0;
    let top = PAGE_H - MARGIN;
    let table_top_first = top - title_size - 10.0 - sub_size - 18.0;
    let bottom = MARGIN + 18.0;

    // A short table on one page gets roomier rows, up to twice the
    // padding, which is easier to read and to write on.
    let lines_of = |cells: &Vec<Vec<String>>| cells.iter().map(Vec::len).max().unwrap_or(1);
    let all_lines: usize = lines_of(&header) + rows.iter().map(lines_of).sum::<usize>();
    let cells = (rows.len() + 1) as f32;
    let spare = (table_top_first - bottom) - (all_lines as f32 * leading + cells * 2.0 * pad_y);
    if spare > 0.0 {
        pad_y += (spare / cells / 2.0).min(pad_y);
    }
    let height = |lines: usize| lines as f32 * leading + 2.0 * pad_y;
    let header_h = height(lines_of(&header));

    // Rows on each page, with the header repeated.
    let mut pages: Vec<Vec<usize>> = vec![Vec::new()];
    let mut y = table_top_first - header_h;
    for (r, cells) in rows.iter().enumerate() {
        let h = height(cells.iter().map(Vec::len).max().unwrap_or(1));
        if y - h < bottom && !pages.last().is_some_and(Vec::is_empty) {
            pages.push(Vec::new());
            y = top - header_h;
        }
        y -= h;
        if let Some(page) = pages.last_mut() {
            page.push(r);
        }
    }

    let count = pages.len();
    let mut contents = Vec::new();
    for (p, page_rows) in pages.iter().enumerate() {
        let mut c = Canvas { ops: Vec::new() };
        let mut y = top;
        if p == 0 {
            c.text(MARGIN, y - title_size * 0.8, &t.title, Font::Bold, title_size, INK);
            y -= title_size + 10.0;
            c.text(MARGIN, y - sub_size * 0.8, &t.subtitle, Font::Regular, sub_size, MUTED);
            y = table_top_first;
        }
        let draw_row = |c: &mut Canvas, y: f32, cells: &[Vec<String>], font: Font, h: f32| {
            let mut x = MARGIN;
            for (i, lines) in cells.iter().enumerate() {
                for (l, line) in lines.iter().enumerate() {
                    let w = text_width(line, font, size);
                    let tx = if t.columns[i].centred { x + (widths[i] - w) / 2.0 } else { x + pad_x };
                    // Lines centred in the row, with the baseline at about 0.8 em down.
                    let block = lines.len() as f32 * leading;
                    let ty = y - (h - block) / 2.0 - l as f32 * leading - (leading + size * 0.7) / 2.0;
                    c.text(tx, ty, line, font, size, INK);
                }
                x += widths[i];
            }
        };
        c.fill_rect(MARGIN, y - header_h, room, header_h, HEAD_FILL);
        draw_row(&mut c, y, &header, Font::Bold, header_h);
        c.line(MARGIN, y - header_h, MARGIN + room, y - header_h, 1.2, RULE);
        y -= header_h;
        for (n, &r) in page_rows.iter().enumerate() {
            let cells = &rows[r];
            let h = height(cells.iter().map(Vec::len).max().unwrap_or(1));
            if n % 2 == 1 {
                c.fill_rect(MARGIN, y - h, room, h, STRIPE);
            }
            draw_row(&mut c, y, cells, Font::Regular, h);
            c.line(MARGIN, y - h, MARGIN + room, y - h, 0.5, RULE);
            y -= h;
        }
        let mut footer = t.footer.clone();
        if count > 1 {
            let _ = write!(footer, "{}Page {} of {count}", if footer.is_empty() { "" } else { " · " }, p + 1);
        }
        if !footer.is_empty() {
            c.text(MARGIN, MARGIN - 4.0, &footer, Font::Regular, 9.0, MUTED);
        }
        contents.push(c.ops);
    }
    write_file(&t.title, &contents)
}

/// The file: catalogue, pages, the two fonts and each page's content.
fn write_file(title: &str, contents: &[Vec<u8>]) -> Vec<u8> {
    // Objects: 1 catalogue, 2 pages, 3 and 4 the fonts, 5 the information,
    // then a page and its content for each page.
    let n = contents.len();
    let page_id = |i: usize| 6 + 2 * i;
    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_id(i))).collect();
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
    for base in ["Helvetica", "Helvetica-Bold"] {
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>").into_bytes());
    }
    let mut info = b"<< /Title ".to_vec();
    info.extend(pdf_string(title));
    info.extend_from_slice(b" /Producer (Supervisor) >>");
    objects.push(info);
    for (i, content) in contents.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                page_id(i) + 1
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }

    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(table, "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.extend(table.into_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rows: usize) -> Table {
        Table {
            title: "Posters to mark".into(),
            subtitle: "2026/27".into(),
            columns: ["Poster", "B number", "Student", "Supervisor", "Course"]
                .iter()
                .map(|h| Column { heading: h.to_string(), centred: *h == "Poster" })
                .collect(),
            rows: (0..rows)
                .map(|i| {
                    vec![
                        (i + 1).to_string(),
                        format!("B0012345{i}"),
                        "José da Silva (Ana)".into(),
                        "Dr Jane Smith".into(),
                        "BEng Hons Mechanical Engineering".into(),
                    ]
                })
                .collect(),
            footer: String::new(),
        }
    }

    #[test]
    fn a_table_makes_a_well_formed_pdf() {
        let pdf = table(&sample(6));
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 1"), "six rows fit on one page");
        // The cross-reference table points at each object.
        let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[start..].starts_with(b"xref"));
        for line in text[start..].lines().skip(3).take_while(|l| l.ends_with(" n ")) {
            let offset: usize = line[..10].parse().unwrap();
            assert!(pdf[offset..].iter().take_while(|b| **b != b'\n').map(|b| *b as char).collect::<String>().ends_with(" 0 obj"));
        }
        // Accented letters are in Windows-1252, and brackets escaped.
        assert!(pdf.windows(4).any(|w| w == b"Jos\xE9"));
        assert!(pdf.windows(7).any(|w| w == b"\\(Ana\\)"));
    }

    #[test]
    fn many_rows_go_onto_more_pages() {
        let pdf = table(&sample(40));
        let text = String::from_utf8_lossy(&pdf);
        assert!(!text.contains("/Count 1 "), "more than one page");
        assert!(text.contains("Page 1 of"));
    }

    #[test]
    fn long_text_wraps_and_long_words_break() {
        assert_eq!(wrap("one two", Font::Regular, 10.0, 1000.0), vec!["one two"]);
        let lines = wrap("Mechanical Engineering", Font::Regular, 10.0, 60.0);
        assert_eq!(lines, vec!["Mechanical", "Engineering"]);
        let broken = wrap("Supercalifragilistic", Font::Regular, 10.0, 30.0);
        assert!(broken.len() > 1 && broken.iter().all(|l| text_width(l, Font::Regular, 10.0) <= 30.0));
        assert_eq!(wrap("", Font::Regular, 10.0, 30.0), vec![String::new()]);
    }
}
