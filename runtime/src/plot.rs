//! `std.plot`: small charts as SVG text.
//!
//! Every function takes the data and a title and gives the text of one SVG image, 640 by 400, with a grid
//! and the numbers on the axes. The output depends only on the arguments (no clock, no random numbers, a
//! fixed way of writing numbers), so the same call writes the same bytes and a report can be compared
//! or kept in version control.

const W: f64 = 640.0;
const H: f64 = 400.0;
/// The plot area inside the image: left, top, right, bottom.
const L: f64 = 64.0;
const T: f64 = 44.0;
const R: f64 = 620.0;
const B: f64 = 340.0;
const COLOR: &str = "#4c78a8";
const MAX_POINTS: usize = 1_000_000;

/// The text with the characters that XML reads as markup replaced.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // Control characters are not allowed in XML.
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A coordinate with at most two decimals and no `-0`, so the text is the same on every machine.
fn num(x: f64) -> String {
    let s = format!("{x:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// The step of a scale: 1, 2 or 5 times a power of ten, giving about five intervals over `range`.
fn nice_step(range: f64) -> f64 {
    let raw = range / 5.0;
    let magnitude = 10f64.powf(raw.log10().floor());
    let norm = raw / magnitude;
    let factor = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

/// The ticks of an axis over `[lo, hi]` (`lo < hi`) and the range the axis covers, which includes both ends.
fn ticks(lo: f64, hi: f64) -> (Vec<f64>, f64, f64) {
    let step = nice_step(hi - lo);
    let start = (lo / step).floor() * step;
    let mut list = Vec::new();
    let mut k = 0.0;
    loop {
        let v = start + k * step;
        list.push(v);
        if v >= hi - step * 1e-9 || list.len() > 64 {
            break;
        }
        k += 1.0;
    }
    let end = *list.last().unwrap_or(&hi);
    (list, start, end)
}

/// A tick label: whole numbers as they are, others with as many decimals as the step needs.
fn tick_label(v: f64, step: f64) -> String {
    let decimals = if step >= 1.0 {
        0
    } else {
        (-step.log10().floor()) as usize
    };
    let s = format!("{v:.decimals$}");
    if s.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        s.trim_start_matches('-').to_string()
    } else {
        s
    }
}

/// An axis range that is never empty: a single value gets room on both sides.
fn widen(lo: f64, hi: f64) -> (f64, f64) {
    if hi > lo {
        (lo, hi)
    } else {
        (lo - 1.0, hi + 1.0)
    }
}

struct Svg {
    body: String,
}

impl Svg {
    /// The frame: background, title, the grid and the labels of the vertical axis, and both axes. `y` is the
    /// range of the vertical axis.
    fn new(title: &str, y_lo: f64, y_hi: f64) -> (Svg, impl Fn(f64) -> f64) {
        let (yt, y_lo, y_hi) = ticks(y_lo, y_hi);
        let step = yt.get(1).map_or(1.0, |b| b - yt[0]);
        let to_y = move |v: f64| B - (v - y_lo) / (y_hi - y_lo) * (B - T);
        let title = escape(title);
        let mut body = String::new();
        body.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" font-family=\"sans-serif\" font-size=\"12\">\n<title>{title}</title>\n<rect width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>\n<text x=\"{cx}\" y=\"26\" text-anchor=\"middle\" font-size=\"16\">{title}</text>\n",
            w = num(W),
            h = num(H),
            cx = num((L + R) / 2.0),
        ));
        for v in &yt {
            let y = num(to_y(*v));
            body.push_str(&format!(
                "<line x1=\"{}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#e5e7eb\"/>\n<text x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>\n",
                num(L),
                num(R),
                num(L - 6.0),
                num(to_y(*v) + 4.0),
                tick_label(*v, step)
            ));
        }
        body.push_str(&format!(
            "<line x1=\"{l}\" y1=\"{t}\" x2=\"{l}\" y2=\"{b}\" stroke=\"#374151\"/>\n<line x1=\"{l}\" y1=\"{b}\" x2=\"{r}\" y2=\"{b}\" stroke=\"#374151\"/>\n",
            l = num(L),
            t = num(T),
            b = num(B),
            r = num(R),
        ));
        (Svg { body }, to_y)
    }

    fn finish(mut self) -> String {
        self.body.push_str("</svg>\n");
        self.body
    }
}

/// Bars, one for each label. A value below zero hangs below the zero line.
pub fn bar(labels: &[String], values: &[f64], title: &str) -> Result<String, String> {
    if labels.len() != values.len() {
        return Err(format!(
            "std.plot.bar: {} label(s) for {} value(s); they have to be the same number",
            labels.len(),
            values.len()
        ));
    }
    if values.is_empty() {
        return Err("std.plot.bar: there is nothing to draw".into());
    }
    if values.len() > MAX_POINTS || values.iter().any(|v| !v.is_finite()) {
        return Err("std.plot.bar: the values have to be finite numbers".into());
    }
    let lo = values.iter().copied().fold(0.0, f64::min);
    let hi = values.iter().copied().fold(0.0, f64::max);
    let (lo, hi) = widen(lo, hi);
    let (mut svg, to_y) = Svg::new(title, lo, hi);
    let n = values.len() as f64;
    let slot = (R - L) / n;
    let width = slot * 0.7;
    let zero = to_y(0.0);
    for (i, (label, v)) in labels.iter().zip(values).enumerate() {
        let x = L + slot * i as f64 + (slot - width) / 2.0;
        let y = to_y(*v);
        let (top, height) = if y <= zero {
            (y, zero - y)
        } else {
            (zero, y - zero)
        };
        svg.body.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{COLOR}\"/>\n",
            num(x),
            num(top),
            num(width),
            num(height)
        ));
        let short: String = if label.chars().count() > 10 {
            label.chars().take(9).chain(std::iter::once('…')).collect()
        } else {
            label.clone()
        };
        svg.body.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>\n",
            num(x + width / 2.0),
            num(B + 18.0),
            escape(&short)
        ));
    }
    Ok(svg.finish())
}

/// One series as a line, `ys` against `xs`.
pub fn line(xs: &[f64], ys: &[f64], title: &str) -> Result<String, String> {
    if xs.len() != ys.len() {
        return Err(format!(
            "std.plot.line: {} x value(s) for {} y value(s); they have to be the same number",
            xs.len(),
            ys.len()
        ));
    }
    if xs.is_empty() {
        return Err("std.plot.line: there is nothing to draw".into());
    }
    if xs.len() > MAX_POINTS || xs.iter().chain(ys).any(|v| !v.is_finite()) {
        return Err("std.plot.line: the values have to be finite numbers".into());
    }
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (y_lo, y_hi) = widen(min(ys), max(ys));
    let (x_lo, x_hi) = widen(min(xs), max(xs));
    let (mut svg, to_y) = Svg::new(title, y_lo, y_hi);
    let (xt, x_lo, x_hi) = ticks(x_lo, x_hi);
    let step = xt.get(1).map_or(1.0, |b| b - xt[0]);
    let to_x = |v: f64| L + (v - x_lo) / (x_hi - x_lo) * (R - L);
    for v in &xt {
        svg.body.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>\n",
            num(to_x(*v)),
            num(B + 18.0),
            tick_label(*v, step)
        ));
    }
    let mut order: Vec<usize> = (0..xs.len()).collect();
    order.sort_by(|a, b| xs[*a].total_cmp(&xs[*b]).then(a.cmp(b)));
    let points: Vec<String> = order
        .iter()
        .map(|&i| format!("{},{}", num(to_x(xs[i])), num(to_y(ys[i]))))
        .collect();
    svg.body.push_str(&format!(
        "<polyline points=\"{}\" fill=\"none\" stroke=\"{COLOR}\" stroke-width=\"2\"/>\n",
        points.join(" ")
    ));
    if xs.len() <= 60 {
        for &i in &order {
            svg.body.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"3\" fill=\"{COLOR}\"/>\n",
                num(to_x(xs[i])),
                num(to_y(ys[i]))
            ));
        }
    }
    Ok(svg.finish())
}

/// The count of values in each of `bins` equal intervals from the smallest to the largest value; the
/// largest value is in the last interval.
pub fn histogram_counts(values: &[f64], bins: usize) -> (Vec<usize>, f64, f64) {
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (lo, hi) = widen(lo, hi);
    let width = (hi - lo) / bins as f64;
    let mut counts = vec![0usize; bins];
    for v in values {
        let i = (((v - lo) / width) as usize).min(bins - 1);
        counts[i] += 1;
    }
    (counts, lo, hi)
}

/// The distribution of `values` in `bins` bars.
pub fn histogram(values: &[f64], bins: i64, title: &str) -> Result<String, String> {
    if values.is_empty() {
        return Err("std.plot.histogram: there is nothing to draw".into());
    }
    if !(1..=1000).contains(&bins) {
        return Err(format!(
            "std.plot.histogram: `bins` is {bins}; it has to be from 1 to 1000"
        ));
    }
    if values.len() > MAX_POINTS || values.iter().any(|v| !v.is_finite()) {
        return Err("std.plot.histogram: the values have to be finite numbers".into());
    }
    let bins = bins as usize;
    let (counts, lo, hi) = histogram_counts(values, bins);
    let peak = counts.iter().copied().max().unwrap_or(1) as f64;
    let (mut svg, to_y) = Svg::new(title, 0.0, peak.max(1.0));
    let (xt, x_lo, x_hi) = ticks(lo, hi);
    let step = xt.get(1).map_or(1.0, |b| b - xt[0]);
    let to_x = |v: f64| L + (v - x_lo) / (x_hi - x_lo) * (R - L);
    for v in &xt {
        svg.body.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>\n",
            num(to_x(*v)),
            num(B + 18.0),
            tick_label(*v, step)
        ));
    }
    let width = (hi - lo) / bins as f64;
    for (i, count) in counts.iter().enumerate() {
        let left = to_x(lo + width * i as f64);
        let right = to_x(lo + width * (i + 1) as f64);
        let y = to_y(*count as f64);
        svg.body.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{COLOR}\" stroke=\"#ffffff\"/>\n",
            num(left),
            num(y),
            num((right - left).max(0.0)),
            num(B - y)
        ));
    }
    Ok(svg.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(svg: &str, needle: &str) -> usize {
        svg.matches(needle).count()
    }

    #[test]
    fn numbers_are_written_the_same_everywhere() {
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.004), "0");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(123.456), "123.46");
        assert_eq!(num(-3.25), "-3.25");
    }

    #[test]
    fn scales_use_round_numbers_and_cover_the_data() {
        let (t, lo, hi) = ticks(0.0, 10.0);
        assert_eq!(
            (t, lo, hi),
            (vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0], 0.0, 10.0)
        );
        let (t, lo, hi) = ticks(0.0, 1.0);
        assert_eq!(t.len(), 6);
        assert!((t[1] - 0.2).abs() < 1e-9 && lo == 0.0 && (hi - 1.0).abs() < 1e-9);
        // The range grows to the next tick on both sides.
        let (t, lo, hi) = ticks(3.0, 97.0);
        assert!(
            lo <= 3.0 && hi >= 97.0 && t[0] == lo && *t.last().unwrap() == hi,
            "{t:?}"
        );
        assert_eq!(tick_label(4.0, 2.0), "4");
        assert_eq!(tick_label(0.2, 0.2), "0.2");
        assert_eq!(tick_label(-0.0, 0.5), "0.0");
    }

    #[test]
    fn markup_in_a_title_or_label_cannot_break_the_image() {
        let svg = bar(
            &["a<b".into(), "c&\"d'".into()],
            &[1.0, 2.0],
            "x</title><script>",
        )
        .unwrap();
        assert!(!svg.contains("<script>") && !svg.contains("a<b"));
        assert!(svg.contains("a&lt;b") && svg.contains("c&amp;&quot;d&#39;"));
        assert!(svg.contains("x&lt;/title&gt;&lt;script&gt;"));
    }

    #[test]
    fn a_bar_chart_has_a_bar_for_each_value_and_is_the_same_each_time() {
        let labels: Vec<String> = ["north", "south", "a-very-long-region-name"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let svg = bar(&labels, &[3.0, -2.0, 5.5], "Sales").unwrap();
        assert!(svg.starts_with("<svg ") && svg.ends_with("</svg>\n"));
        assert_eq!(count(&svg, &format!("fill=\"{COLOR}\"")), 3);
        assert!(svg.contains("a-very-lo…"), "a long label is cut");
        assert_eq!(svg, bar(&labels, &[3.0, -2.0, 5.5], "Sales").unwrap());
        // A negative value is below the zero line and a positive one above it.
        let rects: Vec<&str> = svg.lines().filter(|l| l.contains(COLOR)).collect();
        let attr = |l: &str, name: &str| -> f64 {
            let from = l.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
            l[from..].split('"').next().unwrap().parse().unwrap()
        };
        let zero = attr(rects[0], "y") + attr(rects[0], "height");
        assert!(
            (attr(rects[1], "y") - zero).abs() < 0.02,
            "the negative bar starts at the zero line"
        );
        assert!(attr(rects[2], "y") + attr(rects[2], "height") - zero < 0.02);
    }

    #[test]
    fn bad_input_is_an_error_that_says_what() {
        let e = bar(&["a".into()], &[1.0, 2.0], "t").unwrap_err();
        assert!(e.contains("1 label(s) for 2 value(s)"), "{e}");
        assert!(bar(&[], &[], "t").unwrap_err().contains("nothing to draw"));
        assert!(
            bar(&["a".into()], &[f64::NAN], "t")
                .unwrap_err()
                .contains("finite")
        );
        assert!(
            line(&[1.0], &[1.0, 2.0], "t")
                .unwrap_err()
                .contains("1 x value(s) for 2 y value(s)")
        );
        assert!(
            line(&[f64::INFINITY], &[1.0], "t")
                .unwrap_err()
                .contains("finite")
        );
        assert!(
            histogram(&[1.0], 0, "t")
                .unwrap_err()
                .contains("from 1 to 1000")
        );
        assert!(
            histogram(&[1.0], 1001, "t")
                .unwrap_err()
                .contains("from 1 to 1000")
        );
        assert!(
            histogram(&[], 3, "t")
                .unwrap_err()
                .contains("nothing to draw")
        );
    }

    #[test]
    fn a_histogram_counts_each_value_once_and_puts_the_largest_in_the_last_bar() {
        let values = [1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0, 4.0];
        let (counts, lo, hi) = histogram_counts(&values, 4);
        assert_eq!((counts.clone(), lo, hi), (vec![1, 2, 3, 4], 1.0, 4.0));
        assert_eq!(counts.iter().sum::<usize>(), values.len());
        // All the same value: one range of width 2 around it, nothing divides by zero.
        let (counts, lo, hi) = histogram_counts(&[5.0, 5.0, 5.0], 2);
        assert_eq!((counts.iter().sum::<usize>(), lo, hi), (3, 4.0, 6.0));
        let svg = histogram(&values, 4, "h").unwrap();
        assert_eq!(count(&svg, &format!("fill=\"{COLOR}\"")), 4);
        assert!(histogram(&[5.0, 5.0], 3, "h").is_ok());
    }

    #[test]
    fn a_line_follows_the_order_of_x_and_marks_few_points() {
        let svg = line(&[3.0, 1.0, 2.0], &[30.0, 10.0, 20.0], "l").unwrap();
        let points = svg
            .split("points=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let ys: Vec<f64> = points
            .split(' ')
            .map(|p| p.split(',').nth(1).unwrap().parse().unwrap())
            .collect();
        // x = 1, 2, 3 gives the values 10, 20, 30: the higher the value the smaller the y of the image.
        assert!(ys[0] > ys[1] && ys[1] > ys[2], "{ys:?}");
        assert_eq!(count(&svg, "<circle"), 3);
        let many: Vec<f64> = (0..100).map(f64::from).collect();
        assert_eq!(count(&line(&many, &many, "l").unwrap(), "<circle"), 0);
        // One point, or a flat series, still draws.
        assert!(line(&[1.0], &[1.0], "l").is_ok() && line(&[1.0, 2.0], &[3.0, 3.0], "l").is_ok());
    }
}
