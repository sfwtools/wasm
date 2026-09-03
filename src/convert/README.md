# Unit Converter

Convert common length, mass, temperature, speed, and time values through the
raw ABI. Runtime unit selections dispatch to compile-time typed quantities from
the `uom` crate. Incompatible categories and unknown units are rejected.

The `convert` export takes no input bytes. Its options are `value`, `from`, and
`to`, using the labels in the manifest. It returns JSON containing `value` and
`unit`.

## License

Copyright (C) 2026, Alex Morales
Copyright (C) 2026, sfw.tools sfwtools.com

Licensed under the GNU Affero General Public License version 3.
