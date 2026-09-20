# Bundled spellcheck dictionaries

Hunspell-format dictionaries embedded into the server binary (`include_bytes!` in
`src/routes/dictionaries.rs`) and served at:

- `GET /api/dictionaries/en_US.aff`
- `GET /api/dictionaries/en_US.dic`

Both are public (no session required) and sent with a long `Cache-Control`, because
the bytes for a given file never change — a new dictionary ships as a new file at a
new version key on the client (`dict/en_US/v1/...`), not as a mutation of this one.

They live in the crate rather than being read from the host at runtime so the
Docker / jkbase build needs no extra files: the Dockerfile's `COPY . /build/plotweb/`
carries this directory into the build context, and the release image holds only the
binary.

## Encoding

`en_US.aff` declares `SET UTF-8` on its first line and both files are valid UTF-8 as
shipped. **No transcoding was done at copy time** — they are byte-identical to the
source below. `spellbook::Dictionary::new` takes `&str`, so the client decodes the
fetched bytes as UTF-8 directly. If a future dictionary declares `SET ISO8859-1`,
convert it to UTF-8 here (and rewrite the `SET` line) rather than at load time.

## Source

Copied verbatim from Debian's `hunspell-en-us` package:

- `/usr/share/hunspell/en_US.aff` (3,131 bytes)
- `/usr/share/hunspell/en_US.dic` (860,381 bytes, 79,013 entries)

Upstream is SCOWL (Spell Checker Oriented Word Lists), maintained by Kevin Atkinson.

## License

The collective work is **Copyright 2000-2011 Kevin Atkinson**, under the SCOWL
permissive notice (the full text of the Debian package's
`/usr/share/doc/hunspell-en-us/copyright` is the authoritative version):

> Permission to use, copy, modify, distribute and sell these word lists, the
> associated scripts, the output created from the scripts, and its documentation
> for any purpose is hereby granted without fee, provided that the above copyright
> notice appears in all copies and that both that copyright notice and this
> permission notice appear in supporting documentation. Kevin Atkinson makes no
> representations about the suitability of this array for any purpose. It is
> provided "as is" without express or implied warranty.

SCOWL is assembled from several sources with their own notices, which travel with
it and are reproduced in the Debian copyright file:

- **Moby Words II** (MWords) — explicitly placed in the public domain by Grady Ward.
- **UK English Wordlist with Frequency Classification** (Brian Kelk) — public domain.
- **12Dicts package and Supplement** (Alan Beale) — public domain.
- **WordNet 1.6** — Copyright 1997 Princeton University; permission to use, copy,
  modify and distribute for any purpose without fee or royalty, provided the
  copyright notice and disclaimer accompany all copies. The name of Princeton
  University may not be used in advertising without prior written permission.

All of the above permit redistribution — including within a binary — provided the
notices travel with the word lists. This file is that notice; keep it beside the
dictionaries.
