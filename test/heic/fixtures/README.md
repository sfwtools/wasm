# HEIC fixtures

`orientation.heic`, `irot90.heic`, and `imir_h.heic` are generated feature
fixtures from the `imazen/heic` testdata repository. They are used to verify
HEIF rotation and mirror transforms; see the upstream repository for source
and licensing information.

`exif.heic` is from `dsoprea/heic-exif-samples`, released under the MIT License
by Dustin Oprea. The upstream license is recorded in that repository's
`LICENSE` file.

The Nokia HEIF conformance files are deliberately not copied here because
their repository does not provide an explicit redistribution license. The
sequence test downloads one such file only at test time.
