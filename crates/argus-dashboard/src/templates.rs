//! HTML rendering for the dashboard, using `format!` strings.
//!
//! We deliberately keep this as `format!` rather than askama to avoid a
//! refactor — `askama` is in workspace deps but not used elsewhere in
//! this crate. This is a D2.A decision from the plan: stay minimal.
//!
//! The cohort view is the UX centerpiece: a PR is broken into four
//! named cohorts (slop / security / arch / verdict), each containing
//! navigable layers. Inspired by CodeRabbit's "Change Stack" pattern.

use crate::state::{Cohort, DashboardState, Layer};

/// Render a single cohort as HTML.
pub fn render_cohort(cohort: &Cohort) -> String {
    let layers_html: String = cohort
        .layers
        .iter()
        .map(render_layer)
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<section id="cohort-{id}" class="cohort" data-cohort-id="{id}">
    <h3 class="cohort-name">{icon} {name}</h3>
    <div class="layer-nav">
{layers_html}
    </div>
  </section>"#,
        id = html_escape(&cohort.id),
        name = html_escape(&cohort.name),
        icon = html_escape(&cohort.icon),
        layers_html = layers_html,
    )
}

/// Render a single layer as HTML. `tabindex="0"` makes the article
/// focusable so the J/K handler in `static/app.js` can move focus
/// across the cohort.
pub fn render_layer(layer: &Layer) -> String {
    format!(
        r#"<article id="layer-{id}" class="layer layer-{severity}" tabindex="0" data-layer-id="{id}">
      <header class="layer-header">
        <span class="layer-summary">{summary}</span>
        <span class="layer-file">{file}:{line_start}-{line_end}</span>
      </header>
      <pre class="layer-diff"><code>{diff}</code></pre>
    </article>"#,
        id = html_escape(&layer.id),
        summary = html_escape(&layer.summary),
        file = html_escape(&layer.file),
        line_start = layer.line_start,
        line_end = layer.line_end,
        severity = html_escape(&layer.severity),
        diff = html_escape(&layer.diff_range),
    )
}

/// Render the full dashboard for a PR review. The header is intentionally
/// minimal: the cohort sections are the main content.
pub fn render_dashboard(state: &DashboardState) -> String {
    let cohorts_html: String = state
        .cohorts
        .iter()
        .map(render_cohort)
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>ARGUS — {pr_title}</title>
  <link rel="stylesheet" href="/static/style.css">
  <script src="https://unpkg.com/htmx.org@1.9.10" defer></script>
  <script src="/static/app.js" defer></script>
  <style>
    body {{ font-family: system-ui, sans-serif; max-width: 880px; margin: 40px auto; padding: 0 24px; color: #222; line-height: 1.55; }}
    h1 {{ margin: 0 0 4px; }}
    h3.cohort-name {{ margin: 32px 0 12px; padding-bottom: 4px; border-bottom: 2px solid #ddd; }}
    .cohort {{ margin: 24px 0; }}
    .layer {{ background: #fafafa; border: 1px solid #e5e5e5; border-left: 4px solid #888; border-radius: 4px; padding: 12px 16px; margin: 12px 0; }}
    .layer:focus {{ outline: 3px solid #06c; outline-offset: 2px; }}
    .layer-critical {{ border-left-color: #c00; }}
    .layer-error    {{ border-left-color: #e35; }}
    .layer-warning  {{ border-left-color: #e9a23b; }}
    .layer-info     {{ border-left-color: #6a8; }}
    .layer-header {{ display: flex; justify-content: space-between; gap: 16px; font-size: 14px; }}
    .layer-summary {{ font-weight: 600; }}
    .layer-file {{ color: #888; font-family: ui-monospace, monospace; font-size: 12px; }}
    .layer-diff {{ background: #0d1117; color: #e6edf3; padding: 12px; border-radius: 4px; overflow-x: auto; font-size: 13px; }}
    .kbd {{ display: inline-block; padding: 1px 6px; border: 1px solid #ccc; border-radius: 3px; font-family: ui-monospace, monospace; font-size: 12px; background: #f6f6f6; }}
    a {{ color: #06c; }}
  </style>
</head>
<body>
  <header>
    <h1>ARGUS Review</h1>
    <p>PR: <a href="{pr_url}">{pr_title}</a></p>
    <p>{cohort_count} cohorts, {layer_count} layers &middot; Navigate with <span class="kbd">j</span> <span class="kbd">k</span> <span class="kbd">g</span> <span class="kbd">G</span></p>
  </header>
  <main>
{cohorts_html}
  </main>
</body>
</html>"#,
        pr_title = html_escape(&state.pr_title),
        pr_url = html_escape(&state.pr_url),
        cohort_count = state.cohorts.len(),
        layer_count = state.total_layers(),
        cohorts_html = cohorts_html,
    )
}

/// Minimal HTML escape for safety. Equivalent to the one in `main.rs`,
/// kept private to this module to avoid cross-module coupling.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DashboardState;

    fn layer(id: &str, summary: &str, severity: &str) -> Layer {
        Layer {
            id: id.into(),
            summary: summary.into(),
            file: "src/lib.rs".into(),
            line_start: 10,
            line_end: 20,
            severity: severity.into(),
            diff_range: "+added line".into(),
        }
    }

    fn cohort(id: &str, name: &str, icon: &str, layers: Vec<Layer>) -> Cohort {
        Cohort {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
            layers,
        }
    }

    // ---- Happy path: cohort with 2 layers renders 2 articles ----
    #[test]
    fn cohort_with_two_layers_renders_two_articles() {
        let c = cohort(
            "slop",
            "Aegis Slop",
            "x",
            vec![
                layer("L1", "first finding", "warning"),
                layer("L2", "second finding", "info"),
            ],
        );
        let html = render_cohort(&c);
        assert!(html.contains("id=\"cohort-slop\""));
        assert_eq!(
            html.matches("<article ").count(),
            2,
            "expected exactly 2 <article> elements, got: {}",
            html.matches("<article ").count()
        );
        assert!(
            html.contains("tabindex=\"0\""),
            "articles must be focusable"
        );
    }

    // ---- Edge case: empty cohort renders without panic ----
    #[test]
    fn empty_cohort_renders_without_panic() {
        let c = cohort("sec", "Aegis Security", "s", vec![]);
        let html = render_cohort(&c);
        assert!(html.contains("id=\"cohort-sec\""));
        assert!(html.contains("Aegis Security"));
        assert_eq!(html.matches("<article ").count(), 0);
        // Should still be valid surrounding structure
        assert!(html.contains("<section"));
        assert!(html.contains("</section>"));
    }

    // ---- Regression: render_dashboard wraps all cohorts in <main> ----
    #[test]
    fn render_dashboard_includes_all_cohort_sections() {
        let mut state =
            DashboardState::from_review("https://github.com/o/r/pull/1".into(), "Demo PR".into());
        state.add_cohort(cohort(
            "slop",
            "Aegis Slop",
            "x",
            vec![layer("L1", "f1", "warning")],
        ));
        state.add_cohort(cohort(
            "security",
            "Aegis Security",
            "s",
            vec![layer("L2", "f2", "error")],
        ));
        let html = render_dashboard(&state);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("id=\"cohort-slop\""));
        assert!(html.contains("id=\"cohort-security\""));
        assert!(html.contains("2 cohorts, 2 layers"));
        assert!(html.contains("Demo PR"));
        assert!(html.contains("https://github.com/o/r/pull/1"));
        // The keyboard-nav JS is loaded
        assert!(html.contains("/static/app.js"));
    }

    // ---- A11y: tabindex on every article ----
    #[test]
    fn every_article_has_tabindex_zero() {
        let c = cohort(
            "arch",
            "Aegis Arch",
            "a",
            vec![
                layer("L1", "f1", "info"),
                layer("L2", "f2", "info"),
                layer("L3", "f3", "info"),
            ],
        );
        let html = render_cohort(&c);
        let article_count = html.matches("<article ").count();
        let tabindex_count = html.matches("tabindex=\"0\"").count();
        assert_eq!(article_count, 3);
        assert_eq!(
            tabindex_count, 3,
            "every article must be keyboard-focusable"
        );
    }

    // ---- XSS: special chars are escaped ----
    #[test]
    fn special_chars_in_summary_are_html_escaped() {
        let malicious = "<script>alert(1)</script>";
        let c = cohort(
            "slop",
            "Aegis Slop",
            "x",
            vec![layer("L1", malicious, "warning")],
        );
        let html = render_cohort(&c);
        assert!(
            !html.contains("<script>"),
            "raw <script> must not appear in output: {}",
            html
        );
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "expected escaped payload, got: {}",
            html
        );
    }

    // ---- Extra: empty state renders a valid page ----
    #[test]
    fn dashboard_with_no_cohorts_still_renders() {
        let state = DashboardState::from_review("u".into(), "T".into());
        let html = render_dashboard(&state);
        assert!(html.contains("0 cohorts, 0 layers"));
        assert!(html.contains("<main>"));
        assert!(html.contains("</main>"));
    }

    // ---- Extra: severity class on layer ----
    #[test]
    fn layer_carries_severity_class() {
        let c = cohort(
            "sec",
            "Aegis Security",
            "s",
            vec![layer("L1", "f", "critical")],
        );
        let html = render_cohort(&c);
        assert!(
            html.contains(r#"class="layer layer-critical""#),
            "expected severity in class, got: {}",
            html
        );
    }
}
