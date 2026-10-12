#!/usr/bin/env python3
"""
OctoPad - Custom CSS (GTK4), minimal native-like.
"""

APP_CSS = """
window {
  background-color: @view_bg_color;
}

.mp-page {
  margin: 14px 24px;
}

.mp-chassis {
  background-color: @card_bg_color;
  border: 1px solid @borders;
  border-radius: 12px;
  padding: 20px;
  box-shadow: none;
}

.mp-panel {
  background-color: @card_bg_color;
  border: 1px solid @borders;
  border-radius: 12px;
  padding: 10px;
}

.mp-key {
  min-height: 120px;
  padding: 14px 16px;
  border-radius: 14px;
  border: 1px solid @borders;
  background-color: @view_bg_color;
}

.mp-key:hover {
  background-color: alpha(@view_fg_color, 0.06);
}

.mp-key.mp-key-active {
  border: 3px solid @accent_color;
  background-color: alpha(@accent_color, 0.14);
  box-shadow: 0 0 0 2px alpha(@accent_color, 0.45),
              0 4px 14px alpha(@accent_color, 0.30);
}

.mp-key:active,
.mp-key.mp-key-pressed {
  background-color: alpha(@success_color, 0.14);
  border-color: @success_color;
}

.mp-key-number {
  font-size: 0.82rem;
  font-weight: 800;
  min-width: 24px;
  min-height: 24px;
  border-radius: 999px;
  background-color: alpha(@view_fg_color, 0.10);
  color: alpha(@view_fg_color, 0.85);
}

.mp-key.mp-key-active .mp-key-number {
  background-color: @accent_color;
  color: @accent_fg_color;
}

.mp-key-edit {
  color: @accent_color;
}

.mp-key-label {
  font-size: 0.98rem;
  font-weight: 700;
}

.mp-key-actions {
  margin-top: 0;
}

.mp-key-action-armed.mp-trig-single { color: @accent_color; }
.mp-key-action-armed.mp-trig-double { color: @success_color; }
.mp-key-action-armed.mp-trig-hold   { color: @warning_color; }

.mp-key-action-empty {
  color: alpha(@view_fg_color, 0.20);
}

.mp-accent-icon {
  color: @accent_color;
}

.navigation-sidebar > row {
  border-radius: 8px;
  margin: 2px 6px;
}

.mp-mode-switch > button:checked {
  background-color: @accent_color;
  color: @accent_fg_color;
}

.mp-inspector {
  padding: 14px;
}

.mp-console {
  font-family: monospace;
  font-size: 0.86rem;
}

.mp-pill {
  border-radius: 999px;
  border: 1px solid alpha(@view_fg_color, 0.18);
  background-color: alpha(@view_fg_color, 0.06);
  color: alpha(@view_fg_color, 0.9);
  padding: 0 10px;
}

.mp-pill-dot {
  font-size: 0.7rem;
}

.mp-ok {
  border-color: alpha(@success_color, 0.55);
  background-color: alpha(@success_color, 0.14);
  color: @success_color;
}

.mp-ok .mp-pill-dot {
  color: @success_color;
}

.mp-warn {
  border-color: alpha(@warning_color, 0.55);
  background-color: alpha(@warning_color, 0.14);
  color: @warning_color;
}

.mp-warn .mp-pill-dot {
  color: @warning_color;
}

.mp-err {
  border-color: alpha(@error_color, 0.55);
  background-color: alpha(@error_color, 0.12);
  color: @error_color;
}

.mp-err .mp-pill-dot {
  color: @error_color;
}

.mp-ae-card {
  background-color: alpha(@view_fg_color, 0.03);
  border: 1px solid alpha(@view_fg_color, 0.09);
  border-left-width: 4px;
  border-radius: 10px;
  padding: 8px 10px 8px 12px;
}

.mp-ae-card.mp-ae-single { border-left-color: @accent_color; }
.mp-ae-card.mp-ae-double { border-left-color: @success_color; }
.mp-ae-card.mp-ae-hold   { border-left-color: @warning_color; }

.mp-ae-step {
  font-size: 0.72rem;
  font-weight: 800;
  min-width: 20px;
  border-radius: 999px;
  border: 1px solid alpha(@view_fg_color, 0.14);
  background-color: alpha(@view_fg_color, 0.06);
  padding: 1px 6px;
}

.mp-info {
  min-width: 16px;
  min-height: 16px;
  color: alpha(@view_fg_color, 0.40);
}

.mp-info:hover {
  color: @accent_color;
}

.mp-ae-empty {
  border: 1px dashed alpha(@view_fg_color, 0.22);
  border-radius: 8px;
  padding: 6px 10px;
  color: alpha(@view_fg_color, 0.55);
}

.mp-move {
  min-width: 24px;
  min-height: 24px;
  padding: 2px;
}

/* Trigger colour coding: Single = blue, Double = green, Hold = orange. ---- */
.mp-trigger-title {
  font-weight: 700;
  font-size: 0.86rem;
}

.mp-trig-single { color: @accent_color; }
.mp-trig-double { color: @success_color; }
.mp-trig-hold { color: @warning_color; }

.mp-ae-step.mp-trig-single {
  border-color: alpha(@accent_color, 0.5);
  background-color: alpha(@accent_color, 0.16);
}
.mp-ae-step.mp-trig-double {
  border-color: alpha(@success_color, 0.5);
  background-color: alpha(@success_color, 0.16);
}
.mp-ae-step.mp-trig-hold {
  border-color: alpha(@warning_color, 0.5);
  background-color: alpha(@warning_color, 0.16);
}

.mp-trig-switch > button {
  font-weight: 600;
}
.mp-trig-switch > button.mp-trig-single:checked {
  background-color: alpha(@accent_color, 0.22);
}
.mp-trig-switch > button.mp-trig-double:checked {
  background-color: alpha(@success_color, 0.22);
}
.mp-trig-switch > button.mp-trig-hold:checked {
  background-color: alpha(@warning_color, 0.22);
}

.mp-key-trigger.mp-trig-single {
  color: @accent_color;
  background-color: alpha(@accent_color, 0.12);
}
.mp-key-trigger.mp-trig-double {
  color: @success_color;
  background-color: alpha(@success_color, 0.12);
}
.mp-key-trigger.mp-trig-hold {
  color: @warning_color;
  background-color: alpha(@warning_color, 0.12);
}

.mp-section-title {
  font-weight: 600;
  font-size: 0.78rem;
  color: alpha(@view_fg_color, 0.75);
  margin-top: 4px;
}

.mp-entity {
  background-color: @card_bg_color;
  border: 1px solid @borders;
  border-radius: 10px;
  padding: 10px 12px;
}
"""


def load_css(display) -> None:
    """Attach the application CSS provider to the given display."""
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk

    provider = Gtk.CssProvider()
    provider.load_from_data(APP_CSS.encode("utf-8"))
    # libadwaita loads its stylesheet at PRIORITY_USER (800), which would
    # otherwise override any app styling on Adwaita widgets such as
    # Gtk.Button. Our rules are scoped to mp-* classes, so sitting just above
    # libadwaita is safe and makes the custom card/tab styling apply.
    Gtk.StyleContext.add_provider_for_display(
        display, provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 10
    )
