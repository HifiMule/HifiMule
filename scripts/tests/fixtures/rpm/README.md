# RPM extraction fixture

`rpm-rs-0.16.0.rpm` is a locally generated, unsigned, gzip-compressed test package.
It contains `/usr/share/hifimule-fixture/payload.txt` with `RPM extraction regression` and a newline.
It contains no executables or third-party package contents.

Generated with `rpm = { version = "=0.16.0", default-features = false, features = ["gzip-compression"] }`, the RPM library version locked by Tauri CLI 2.12.1:

```rust
let pkg = rpm::PackageBuilder::new(
    "hifimule-extraction-fixture", "1.0.0", "MIT", "noarch", "Extraction fixture",
)
.with_file("payload.txt", rpm::FileOptions::new("/usr/share/hifimule-fixture/payload.txt"))?
.build()?;
pkg.write_file("fixture.rpm")?;
```

SHA-256: `57886c7ed2136f563e42a1c9386bf5cd5263e02e3aad97ae07fd2ab274adc154`.
Regenerating may change timestamps and the digest without changing the extraction behavior.

Ubuntu 22.04 RPM 4.17.0 reproduces `rpm2cpio=1`, `cpio=0` on this package.
The older extractor compares copied payload bytes against archive-size metadata absent from this builder.
Libarchive's `bsdtar` extracts the expected file successfully.
