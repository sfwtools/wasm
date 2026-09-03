# Datetime

Calculate differences and perform calendar arithmetic on ISO dates and RFC
3339 timestamps. The module exports `difference`, `add`, `subtract`, and
`calendar_info` through the raw ABI.

Months and years are calendar units. For example, adding one month to
`2026-03-09` returns `2026-04-09`; dates that do not exist in the destination
month are clamped to that month's last day.

## License

Copyright (C) 2026, Alex Morales
Copyright (C) 2026, sfw.tools sfwtools.com

Licensed under the GNU Affero General Public License version 3.
