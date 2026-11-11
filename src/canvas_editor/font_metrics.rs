//! Glyph widths shared by every editor in the process.
//!
//! Measuring the wide reference glyph can start a fallback-font search that
//! costs milliseconds, so each font and size is measured once.

use iced::Font;
use iced::advanced::text::{
    Alignment, Paragraph, Renderer as TextRenderer, Text,
};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use super::FONT_METRICS_EPSILON;

/// Narrow and wide reference glyph widths for one font and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CharMetrics {
    /// Width of a narrow character (`a`).
    pub(crate) char_width: f32,
    /// Width of a wide character such as CJK (`汉`).
    pub(crate) full_char_width: f32,
}

type Key = (Font, u32);

#[cfg(test)]
thread_local! {
    /// Full measurements taken on this thread, counted by the tests.
    pub(crate) static MEASUREMENTS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

fn cache() -> MutexGuard<'static, HashMap<Key, CharMetrics>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, CharMetrics>>> = OnceLock::new();
    CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Returns the metrics of `font` at `size`, measuring them on first use.
pub(crate) fn cached(font: Font, size: f32) -> CharMetrics {
    let key = (font, size.to_bits());
    if let Some(metrics) = cache().get(&key) {
        return *metrics;
    }
    store(key, measure(font, size))
}

/// Returns fresh metrics after the live narrow width turned out to be
/// `char_width`, reusing an entry another editor already refreshed.
pub(crate) fn refresh(font: Font, size: f32, char_width: f32) -> CharMetrics {
    let key = (font, size.to_bits());
    let refreshed = cache().get(&key).copied().filter(|metrics| {
        (metrics.char_width - char_width).abs() <= FONT_METRICS_EPSILON
    });
    refreshed.unwrap_or_else(|| store(key, measure(font, size)))
}

fn store(key: Key, metrics: CharMetrics) -> CharMetrics {
    cache().insert(key, metrics);
    metrics
}

fn measure(font: Font, size: f32) -> CharMetrics {
    #[cfg(test)]
    MEASUREMENTS.with(|count| count.set(count.get() + 1));
    let width = |content, fallback| {
        let width = glyph_width(font, size, content);
        if width.is_infinite() { fallback } else { width }
    };
    CharMetrics {
        char_width: width("a", size / 2.0),
        full_char_width: width("汉", size),
    }
}

/// Measures the rendered width of `content` in `font` at `size`.
pub(crate) fn glyph_width(font: Font, size: f32, content: &str) -> f32 {
    let text = Text {
        content,
        font,
        size: iced::Pixels(size),
        line_height: iced::advanced::text::LineHeight::default(),
        bounds: iced::Size::new(f32::INFINITY, f32::INFINITY),
        align_x: Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: iced::advanced::text::Shaping::Advanced,
        wrapping: iced::advanced::text::Wrapping::default(),
    };
    <iced::Renderer as TextRenderer>::Paragraph::with_text(text).min_width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_editor::CodeEditor;

    fn measurements() -> usize {
        MEASUREMENTS.with(std::cell::Cell::get)
    }

    /// The setup Rustman runs: construct, then pick the font and its size.
    fn configured_editor(font: Font, size: f32) -> CodeEditor {
        let mut editor = CodeEditor::new("{}", "json");
        editor.set_font(font);
        editor.set_font_size(size, true);
        editor
    }

    #[test]
    fn test_cached_metrics_match_a_direct_measurement() {
        let font = Font::with_name("Metrics Match Test");
        assert_eq!(cached(font, 15.5), measure(font, 15.5));
        assert_eq!(cached(font, 15.5), measure(font, 15.5));
    }

    #[test]
    fn test_editor_setup_measures_each_font_and_size_once() {
        let font = Font::with_name("Metrics Count Test");
        let start = measurements();

        let first = configured_editor(font, 13.25);
        assert_eq!(measurements(), start, "setup alone measures nothing");
        let metrics = first.char_metrics();
        assert_eq!(measurements(), start + 1);

        for _ in 0..8 {
            let editor = configured_editor(font, 13.25);
            assert_eq!(editor.char_metrics(), metrics);
        }
        assert_eq!(measurements(), start + 1, "same font and size: cache hit");

        let larger = configured_editor(font, 16.75);
        let _ = larger.char_width();
        let _ = configured_editor(font, 16.75).full_char_width();
        assert_eq!(measurements(), start + 2, "a new size measures once");
    }

    #[test]
    fn test_self_heal_updates_the_shared_cache_after_font_registration() {
        let font = Font::with_name("Metrics Heal Test");
        let size = 14.5;
        let real = measure(font, size);
        let fallback = CharMetrics {
            char_width: real.char_width + 2.0,
            full_char_width: real.full_char_width + 4.0,
        };
        // Measured before the real font was registered.
        let _ = store((font, size.to_bits()), fallback);

        let healing = configured_editor(font, size);
        let stale = configured_editor(font, size);
        assert_eq!(healing.char_metrics(), fallback);
        assert_eq!(stale.char_metrics(), fallback);

        let start = measurements();
        healing.ensure_font_metrics();
        assert_eq!(healing.char_metrics(), real);
        assert_eq!(measurements(), start + 1);

        stale.ensure_font_metrics();
        assert_eq!(stale.char_metrics(), real);
        assert_eq!(
            configured_editor(font, size).char_metrics(),
            real,
            "later editors start from the healed entry"
        );
        assert_eq!(measurements(), start + 1);
    }
}
