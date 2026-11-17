use std::fmt::Write as _;

const W: f32 = 1024.0;
const H: f32 = 1024.0;
const STROKE: &str = "#202633";
const BG: &str = "#e9e0cf";
const SHADOW: &str = "#d8c7aa";
const ORANGE: &str = "#f45b00";
const ORANGE_DARK: &str = "#de5200";
const ORANGE_LIGHT: &str = "#ff9d54";
const PEACH: &str = "#f7a061";
const TEAL: &str = "#2d726d";
const TEAL_DARK: &str = "#214f58";
const PAPER: &str = "#f7f2de";

/// Versão vetorial do caranguejo leitor com fundo bege e sombra.
pub fn svg_com_fundo() -> String {
    build_svg(true)
}

/// Versão com fundo e sombra removidos para uso como ícone.
pub fn svg() -> String {
    build_svg(false)
}

fn build_svg(with_bg: bool) -> String {
    let mut s = String::new();
    writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#).unwrap();
    writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">"#).unwrap();

    if with_bg {
        rect(&mut s, 0.0, 0.0, W, H, BG, None, 0.0, None);
        ellipse(&mut s, 512.0, 764.0, 424.0, 52.0, SHADOW, None, 0.0, None);
    }

    draw_shell(&mut s);
    draw_face(&mut s);
    draw_rear_legs(&mut s);
    draw_body_peek(&mut s);
    draw_book(&mut s);
    draw_front_claws(&mut s);
    draw_front_legs(&mut s);

    s.push_str("</svg>\n");
    s
}

fn draw_shell(s: &mut String) {
    // jagged top shell approximating the reference image
    let pts = [
        (270.0, 491.0), (286.0, 470.0), (271.0, 433.0), (298.0, 417.0),
        (290.0, 383.0), (323.0, 372.0), (320.0, 337.0), (357.0, 339.0),
        (362.0, 300.0), (401.0, 311.0), (416.0, 272.0), (456.0, 290.0),
        (482.0, 254.0), (512.0, 286.0), (543.0, 253.0), (573.0, 287.0),
        (608.0, 258.0), (627.0, 293.0), (666.0, 275.0), (682.0, 309.0),
        (719.0, 298.0), (722.0, 338.0), (759.0, 340.0), (758.0, 375.0),
        (791.0, 385.0), (783.0, 417.0), (811.0, 432.0), (795.0, 468.0),
        (807.0, 490.0), (790.0, 518.0), (800.0, 546.0), (774.0, 566.0),
        (778.0, 600.0), (739.0, 604.0), (727.0, 636.0), (685.0, 628.0),
        (666.0, 660.0), (627.0, 645.0), (607.0, 672.0), (563.0, 655.0),
        (534.0, 679.0), (493.0, 659.0), (457.0, 678.0), (427.0, 654.0),
        (387.0, 665.0), (370.0, 638.0), (329.0, 646.0), (315.0, 614.0),
        (283.0, 609.0), (290.0, 575.0), (255.0, 563.0), (263.0, 531.0),
    ];
    let mut d = String::new();
    write!(d, "M {:.1} {:.1}", pts[0].0, pts[0].1).unwrap();
    for i in 1..pts.len() {
        let p0 = pts[i - 1];
        let p1 = pts[i];
        let mx = (p0.0 + p1.0) / 2.0;
        let my = (p0.1 + p1.1) / 2.0;
        write!(d, " Q {:.1} {:.1} {:.1} {:.1}", p0.0, p0.1, mx, my).unwrap();
    }
    let p_last = pts[pts.len() - 1];
    let p_first = pts[0];
    let mx = (p_last.0 + p_first.0) / 2.0;
    let my = (p_last.1 + p_first.1) / 2.0;
    write!(d, " Q {:.1} {:.1} {:.1} {:.1} Z", p_last.0, p_last.1, mx, my).unwrap();

    path(s, &d, ORANGE, Some(STROKE), 8.0, None);

    // lower right shadow crescent
    path(
        s,
        "M 598 630 C 694 621, 761 560, 781 470 C 786 435, 780 401, 770 372 C 732 518, 655 593, 598 630 Z",
        ORANGE_DARK,
        None,
        0.0,
        Some(0.38),
    );
}

fn draw_face(s: &mut String) {
    // glasses
    circle(s, 379.0, 446.0, 79.0, "none", Some(STROKE), 8.0, None);
    circle(s, 573.0, 445.0, 79.0, "none", Some(STROKE), 8.0, None);
    path(s, "M 451 449 Q 476 430 501 448", "none", Some(STROKE), 8.0, None);
    path(s, "M 648 444 Q 721 421 751 413", "none", Some(STROKE), 8.0, None);
    path(s, "M 749 413 Q 770 405 778 422", "none", Some(STROKE), 8.0, None);

    circle(s, 379.0, 446.0, 61.0, PEACH, None, 0.0, None);
    circle(s, 573.0, 445.0, 61.0, PEACH, None, 0.0, None);

    ellipse(s, 369.0, 452.0, 31.0, 46.0, "#000000", None, 0.0, None);
    ellipse(s, 567.0, 452.0, 31.0, 46.0, "#000000", None, 0.0, None);
    circle(s, 404.0, 421.0, 12.0, "#f7f2de", None, 0.0, None);
    circle(s, 570.0, 421.0, 12.0, "#f7f2de", None, 0.0, None);

    path(s, "M 450 513 Q 473 534 500 513", ORANGE, Some(STROKE), 8.0, None);
    path(s, "M 450 513 Q 474 533 499 512", "none", Some(STROKE), 8.0, None);
}

fn draw_rear_legs(s: &mut String) {
    // right side legs behind book/claw
    leg(s, &[(720.0, 603.0), (782.0, 571.0), (852.0, 613.0), (812.0, 720.0)], true);
    leg(s, &[(748.0, 624.0), (817.0, 618.0), (869.0, 670.0), (818.0, 782.0)], true);
    leg(s, &[(690.0, 639.0), (739.0, 641.0), (786.0, 703.0), (740.0, 795.0)], true);

    // body curve on right underside
    path(s, "M 562 609 C 627 589, 683 603, 739 653", "none", Some(STROKE), 8.0, None);
}

fn draw_body_peek(s: &mut String) {
    // visible pale body behind book
    path(
        s,
        "M 572 588 C 610 575, 653 584, 679 606 C 657 629, 619 644, 571 646 C 558 625, 555 605, 572 588 Z",
        "#f2b68f",
        Some(STROKE),
        8.0,
        None,
    );
}

fn draw_book(s: &mut String) {
    // back claw on left
    path(
        s,
        "M 106 588 C 109 544, 142 521, 177 534 C 198 543, 209 564, 215 587 C 220 611, 210 631, 185 637 C 146 648, 104 633, 106 588 Z",
        ORANGE,
        Some(STROKE),
        8.0,
        None,
    );

    // left cover
    path(
        s,
        "M 129 448 L 316 561 L 380 797 L 220 710 Z",
        TEAL,
        Some(STROKE),
        8.0,
        None,
    );
    // inner left page
    path(
        s,
        "M 145 462 L 327 568 L 351 536 L 179 434 Z",
        PAPER,
        Some(STROKE),
        8.0,
        None,
    );
    // spine
    path(
        s,
        "M 316 561 L 412 531 L 487 761 C 495 788, 482 806, 456 808 C 430 810, 404 804, 380 797 Z",
        TEAL_DARK,
        Some(STROKE),
        8.0,
        None,
    );
    // right cover
    path(
        s,
        "M 412 531 L 613 559 L 625 756 L 487 761 Z",
        TEAL,
        Some(STROKE),
        8.0,
        None,
    );
    // top page edge on right
    path(
        s,
        "M 328 567 L 613 559 L 582 520 L 350 533 Z",
        PAPER,
        Some(STROKE),
        8.0,
        None,
    );
}

fn draw_front_claws(s: &mut String) {
    // right arm / forelimb
    path(
        s,
        "M 611 645 C 629 589, 677 571, 708 575 C 720 578, 736 589, 743 603 C 732 634, 692 661, 648 666 C 631 666, 618 659, 611 645 Z",
        ORANGE,
        Some(STROKE),
        8.0,
        None,
    );

    // left front claw over book
    path(
        s,
        "M 512 710 C 539 639, 610 619, 672 633 C 714 643, 736 676, 731 715 C 723 777, 656 805, 583 787 C 537 776, 499 746, 512 710 Z",
        ORANGE,
        Some(STROKE),
        8.0,
        None,
    );
    path(
        s,
        "M 620 639 C 611 665, 573 697, 529 708 C 551 651, 606 628, 620 639 Z",
        ORANGE,
        Some(STROKE),
        8.0,
        None,
    );
}

fn draw_front_legs(s: &mut String) {
    // small connecting leg on left behind book
    leg(s, &[(198.0, 616.0), (262.0, 610.0), (288.0, 673.0), (264.0, 791.0)], false);
    // front visible left leg
    leg(s, &[(149.0, 684.0), (192.0, 676.0), (207.0, 734.0), (195.0, 903.0)], false);
    // front central left leg
    leg(s, &[(271.0, 744.0), (315.0, 742.0), (333.0, 807.0), (337.0, 898.0)], false);
}

fn leg(s: &mut String, pts: &[(f32, f32); 4], right_side: bool) {
    let fill = ORANGE;
    let d = format!(
        "M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1} L {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1} Z",
        pts[0].0, pts[0].1,
        pts[1].0, pts[1].1,
        pts[2].0, pts[2].1,
        (pts[2].0 + pts[3].0) / 2.0, (pts[2].1 + pts[3].1) / 2.0,
        pts[3].0, pts[3].1,
        pts[3].0 - if right_side { 18.0 } else { 14.0 }, pts[3].1 + 3.0,
        (pts[0].0 + pts[3].0) / 2.0 - if right_side { 12.0 } else { 7.0 }, (pts[0].1 + pts[3].1) / 2.0,
        pts[0].0, pts[0].1,
    );
    path(s, &d, fill, Some(STROKE), 8.0, None);
    // segment stripe
    let stripe = format!(
        "M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Z",
        (pts[0].0 + pts[1].0) / 2.0,
        (pts[0].1 + pts[1].1) / 2.0,
        (pts[0].0 + pts[1].0) / 2.0 + if right_side { 37.0 } else { 26.0 },
        (pts[0].1 + pts[1].1) / 2.0 + if right_side { 26.0 } else { 18.0 },
        (pts[0].0 + pts[1].0) / 2.0 + if right_side { 19.0 } else { 9.0 },
        (pts[0].1 + pts[1].1) / 2.0 + if right_side { 41.0 } else { 34.0 },
        (pts[0].0 + pts[1].0) / 2.0 - if right_side { 8.0 } else { 7.0 },
        (pts[0].1 + pts[1].1) / 2.0 + if right_side { 18.0 } else { 16.0 },
    );
    path(s, &stripe, ORANGE_DARK, None, 0.0, Some(0.85));
}

fn rect(
    s: &mut String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill: &str,
    stroke: Option<&str>,
    stroke_width: f32,
    opacity: Option<f32>,
) {
    let op = opacity.map(|o| format!(r#" opacity="{:.3}""#, o)).unwrap_or_default();
    let st = stroke.map(|c| format!(r#" stroke="{}" stroke-width="{:.1}" stroke-linejoin="round""#, c, stroke_width)).unwrap_or_default();
    writeln!(s, r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}"{}{} />"#, x, y, w, h, fill, st, op).unwrap();
}

fn circle(
    s: &mut String,
    cx: f32,
    cy: f32,
    r: f32,
    fill: &str,
    stroke: Option<&str>,
    stroke_width: f32,
    opacity: Option<f32>,
) {
    let op = opacity.map(|o| format!(r#" opacity="{:.3}""#, o)).unwrap_or_default();
    let st = stroke.map(|c| format!(r#" stroke="{}" stroke-width="{:.1}" stroke-linejoin="round""#, c, stroke_width)).unwrap_or_default();
    writeln!(s, r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}"{}{} />"#, cx, cy, r, fill, st, op).unwrap();
}

fn ellipse(
    s: &mut String,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    fill: &str,
    stroke: Option<&str>,
    stroke_width: f32,
    opacity: Option<f32>,
) {
    let op = opacity.map(|o| format!(r#" opacity="{:.3}""#, o)).unwrap_or_default();
    let st = stroke.map(|c| format!(r#" stroke="{}" stroke-width="{:.1}" stroke-linejoin="round""#, c, stroke_width)).unwrap_or_default();
    writeln!(s, r#"<ellipse cx="{:.1}" cy="{:.1}" rx="{:.1}" ry="{:.1}" fill="{}"{}{} />"#, cx, cy, rx, ry, fill, st, op).unwrap();
}

fn path(
    s: &mut String,
    d: &str,
    fill: &str,
    stroke: Option<&str>,
    stroke_width: f32,
    opacity: Option<f32>,
) {
    let op = opacity.map(|o| format!(r#" opacity="{:.3}""#, o)).unwrap_or_default();
    let st = stroke.map(|c| format!(r#" stroke="{}" stroke-width="{:.1}" stroke-linecap="round" stroke-linejoin="round""#, c, stroke_width)).unwrap_or_default();
    writeln!(s, r#"<path d="{}" fill="{}"{}{} />"#, d, fill, st, op).unwrap();
}
