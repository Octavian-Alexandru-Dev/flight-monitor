use std::fmt::Write;

use chrono::{DateTime, Local, NaiveDate, Utc};
use domain::{CalendarCell, Decimal};
use rust_decimal::prelude::ToPrimitive;

use super::views::{day_short, money};

const WIDTH: f64 = 1080.0;
const HEIGHT: f64 = 260.0;
const LEFT: f64 = 56.0;
const RIGHT: f64 = 84.0;
const TOP: f64 = 18.0;
const BOTTOM: f64 = 30.0;
const HEAT_CLASSES: usize = 5;

fn escape_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn f(value: Decimal) -> f64 {
    value.to_f64().unwrap_or(0.0)
}

/// Clean tick values covering `[min, max]`, about four of them.
fn ticks(min: f64, max: f64) -> Vec<f64> {
    let span = (max - min).max(1.0);
    let raw = span / 4.0;
    let magnitude = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|s| span / s <= 5.0)
        .unwrap_or(magnitude * 10.0);
    let mut tick = (min / step).floor() * step;
    let mut out = vec![tick];
    while tick < max {
        tick += step;
        out.push(tick);
    }
    out
}

fn compact(value: f64) -> String {
    if value >= 10_000.0 {
        format!("{:.0}K", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

/// Best price per run over time: one series, crosshair data for the hover layer.
pub fn history_chart(points: &[(DateTime<Utc>, Decimal)], currency: &str) -> Option<String> {
    if points.len() < 2 {
        return None;
    }
    let prices: Vec<f64> = points.iter().map(|(_, p)| f(*p)).collect();
    let (lo, hi) = prices
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let pad = ((hi - lo) * 0.12).max(hi * 0.02).max(1.0);
    let ticks = ticks((lo - pad).max(0.0), hi + pad);
    let (y_min, y_max) = (ticks[0], ticks[ticks.len() - 1].max(ticks[0] + 1.0));
    let t0 = points[0].0.timestamp() as f64;
    let t1 = points[points.len() - 1].0.timestamp() as f64;
    let plot_w = WIDTH - LEFT - RIGHT;
    let plot_h = HEIGHT - TOP - BOTTOM;
    let x = |t: f64| {
        if t1 > t0 {
            LEFT + (t - t0) / (t1 - t0) * plot_w
        } else {
            LEFT + plot_w / 2.0
        }
    };
    let y = |p: f64| TOP + (1.0 - (p - y_min) / (y_max - y_min)) * plot_h;
    let coords: Vec<(f64, f64)> = points
        .iter()
        .map(|(at, p)| (x(at.timestamp() as f64), y(f(*p))))
        .collect();

    let mut svg = String::new();
    let last = prices[prices.len() - 1];
    let label = format!(
        "Prezzo migliore nel tempo: da {} a {}, minimo {}",
        money(points[0].1, currency),
        money(points[points.len() - 1].1, currency),
        money(Decimal::from_f64_retain(lo).unwrap_or_default(), currency)
    );
    let data: Vec<String> = points
        .iter()
        .zip(&coords)
        .map(|((at, price), (cx, cy))| {
            let when = at.with_timezone(&Local).format("%d/%m %H:%M");
            format!(
                "[{cx:.1},{cy:.1},\"{when}\",\"{}\"]",
                money(*price, currency)
            )
        })
        .collect();
    let _ = write!(
        svg,
        r#"<svg class="chart" viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-label="{}" tabindex="0" data-points="{}">"#,
        escape_attr(&label),
        escape_attr(&format!("[{}]", data.join(",")))
    );
    for tick in &ticks {
        let ty = y(*tick);
        let _ = write!(
            svg,
            r#"<line class="grid" x1="{LEFT}" x2="{}" y1="{ty:.1}" y2="{ty:.1}"/><text class="tick" x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
            WIDTH - RIGHT,
            LEFT - 8.0,
            ty + 4.0,
            compact(*tick)
        );
    }
    let base = TOP + plot_h;
    let _ = write!(
        svg,
        r#"<line class="axis" x1="{LEFT}" x2="{}" y1="{base}" y2="{base}"/>"#,
        WIDTH - RIGHT
    );
    let first_day = points[0].0.with_timezone(&Local).date_naive();
    let last_day = points[points.len() - 1]
        .0
        .with_timezone(&Local)
        .date_naive();
    let _ = write!(
        svg,
        r#"<text class="tick" x="{LEFT}" y="{}" text-anchor="start">{}</text>"#,
        HEIGHT - 8.0,
        day_short(first_day)
    );
    if last_day != first_day {
        let _ = write!(
            svg,
            r#"<text class="tick" x="{}" y="{}" text-anchor="end">{}</text>"#,
            WIDTH - RIGHT,
            HEIGHT - 8.0,
            day_short(last_day)
        );
    }
    if coords.len() > 1 {
        let line: Vec<String> = coords
            .iter()
            .map(|(cx, cy)| format!("{cx:.1},{cy:.1}"))
            .collect();
        let (fx, _) = coords[0];
        let (lx, _) = coords[coords.len() - 1];
        let _ = write!(
            svg,
            r#"<path class="area" d="M{fx:.1},{base} L{} L{lx:.1},{base} Z"/><polyline class="line" points="{}"/>"#,
            line.join(" L"),
            line.join(" ")
        );
    }
    let min_index = prices
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    let (lx, ly) = coords[coords.len() - 1];
    if min_index != coords.len() - 1 && (prices[min_index] - last).abs() > f64::EPSILON {
        let (mx, my) = coords[min_index];
        let _ = write!(
            svg,
            r#"<circle class="dot muted-dot" cx="{mx:.1}" cy="{my:.1}" r="4"/><text class="mark-label" x="{mx:.1}" y="{:.1}" text-anchor="middle">min {}</text>"#,
            (my + 18.0).min(base - 4.0),
            money(points[min_index].1, currency)
        );
    }
    let _ = write!(
        svg,
        r#"<circle class="dot" cx="{lx:.1}" cy="{ly:.1}" r="4"/><text class="end-label" x="{:.1}" y="{:.1}">{}</text>"#,
        lx + 10.0,
        ly + 4.0,
        money(points[points.len() - 1].1, currency)
    );
    let _ = write!(
        svg,
        r#"<line class="crosshair" x1="0" x2="0" y1="{TOP}" y2="{base}" visibility="hidden"/>"#
    );
    svg.push_str("</svg>");
    Some(svg)
}

/// Tiny trend line for list rows: de-emphasised, latest point in the accent.
pub fn sparkline(prices: &[Decimal]) -> Option<String> {
    const W: f64 = 112.0;
    const H: f64 = 28.0;
    if prices.is_empty() {
        return None;
    }
    let values: Vec<f64> = prices.iter().map(|p| f(*p)).collect();
    let (lo, hi) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let span = (hi - lo).max(1.0);
    let step = if values.len() > 1 {
        (W - 8.0) / (values.len() - 1) as f64
    } else {
        0.0
    };
    let coords: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            (
                4.0 + i as f64 * step,
                4.0 + (1.0 - (v - lo) / span) * (H - 8.0),
            )
        })
        .collect();
    let (lx, ly) = coords[coords.len() - 1];
    let lx = if values.len() == 1 { W / 2.0 } else { lx };
    let line: Vec<String> = coords
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect();
    Some(format!(
        r#"<svg class="spark" viewBox="0 0 {W} {H}" width="{W}" height="{H}" aria-hidden="true"><polyline points="{}"/><circle cx="{lx:.1}" cy="{ly:.1}" r="3"/></svg>"#,
        line.join(" ")
    ))
}

/// Price calendar as an accessible table on a one-hue scale, cheapest at the strong end.
pub fn heatmap(cells: &[CalendarCell], currency: &str) -> Option<String> {
    if cells.is_empty() {
        return None;
    }
    let mut outs: Vec<NaiveDate> = cells.iter().map(|c| c.outbound).collect();
    outs.sort();
    outs.dedup();
    let mut backs: Vec<Option<NaiveDate>> = cells.iter().map(|c| c.inbound).collect();
    backs.sort();
    backs.dedup();
    let mut sorted: Vec<Decimal> = cells.iter().map(|c| c.price).collect();
    sorted.sort();
    let cheapest = sorted[0];
    let class_of = |price: Decimal| {
        let rank = sorted.partition_point(|p| *p < price);
        rank * HEAT_CLASSES / sorted.len()
    };
    let round_trip = backs.iter().any(Option::is_some);
    let mut html = String::from(r#"<div class="heatmap-scroll"><table class="heatmap">"#);
    let _ = write!(
        html,
        "<caption class=\"sr-only\">Prezzo migliore per data di {}</caption><thead><tr><th scope=\"col\">{}</th>",
        if round_trip {
            "andata e ritorno"
        } else {
            "partenza"
        },
        if round_trip {
            "Andata / ritorno"
        } else {
            "Partenza"
        }
    );
    if round_trip {
        for back in backs.iter().flatten() {
            let _ = write!(html, r#"<th scope="col">{}</th>"#, day_short(*back));
        }
    } else {
        html.push_str(r#"<th scope="col">Prezzo</th>"#);
    }
    html.push_str("</tr></thead><tbody>");
    for out in &outs {
        let _ = write!(html, r#"<tr><th scope="row">{}</th>"#, day_short(*out));
        for back in &backs {
            match cells
                .iter()
                .find(|c| c.outbound == *out && c.inbound == *back)
            {
                Some(cell) => {
                    let tip = match back {
                        Some(b) => format!(
                            "{} → {}: {}",
                            day_short(*out),
                            day_short(*b),
                            money(cell.price, currency)
                        ),
                        None => format!("{}: {}", day_short(*out), money(cell.price, currency)),
                    };
                    let best = if cell.price == cheapest { " best" } else { "" };
                    let _ = write!(
                        html,
                        r#"<td class="q{}{best}" tabindex="0" data-tip="{}">{}</td>"#,
                        class_of(cell.price),
                        escape_attr(&tip),
                        money(cell.price, currency)
                    );
                }
                None => html.push_str(r#"<td class="none" aria-label="nessun dato"></td>"#),
            }
        }
        html.push_str("</tr>");
    }
    let most = sorted[sorted.len() - 1];
    let _ = write!(
        html,
        r#"</tbody></table></div><div class="heat-legend"><span class="muted">economico</span><span>{}</span><span class="heat-ramp" aria-hidden="true"><i class="q0"></i><i class="q1"></i><i class="q2"></i><i class="q3"></i><i class="q4"></i></span><span>{}</span><span class="muted">caro</span></div>"#,
        money(cheapest, currency),
        money(most, currency)
    );
    Some(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_numbers() {
        assert_eq!(
            ticks(420.0, 610.0),
            [400.0, 450.0, 500.0, 550.0, 600.0, 650.0]
        );
    }

    #[test]
    fn heatmap_marks_cheapest_cell() {
        let cell = |o: &str, b: &str, p: i64| CalendarCell {
            outbound: o.parse().unwrap(),
            inbound: Some(b.parse().unwrap()),
            price: Decimal::from(p),
        };
        let html = heatmap(
            &[
                cell("2026-12-10", "2026-12-18", 700),
                cell("2026-12-10", "2026-12-19", 720),
                cell("2026-12-11", "2026-12-18", 650),
            ],
            "EUR",
        )
        .unwrap();
        assert!(html.contains(r#"class="q0 best""#));
        assert!(html.contains("nessun dato"));
    }

    #[test]
    fn chart_escapes_point_data() {
        let now = Utc::now();
        assert!(history_chart(&[(now, Decimal::from(500))], "EUR").is_none());
        let svg = history_chart(
            &[(now, Decimal::from(500)), (now, Decimal::from(450))],
            "EUR",
        )
        .unwrap();
        assert!(svg.contains("data-points=\"[["));
        assert!(svg.contains("&quot;"));
    }
}
