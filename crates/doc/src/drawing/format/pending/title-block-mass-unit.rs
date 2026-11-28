//! A title block's text cell can save `mass_unit` (`kg`, `g`, `auto` or `both`) to choose how
//! `component.mass` is shown in that cell. A cell saved without `mass_unit` reads as `kg` and saves
//! without it, so older cells need no upgrade step. This change still bumps the drawing format
//! number: a drawing saved by this build carries the new `format` value and its `format_pending`
//! list, even when no cell has `mass_unit`. The last release warns about the newer drawing format,
//! opens what it can read and keeps the rest.
