# playfair

The FairPlay key decryption AirPlay mirroring needs: an iPhone sends the
stream's AES key wrapped with FairPlay, and these functions unwrap it.

Taken unchanged from [UxPlay](https://github.com/FDH2/UxPlay) (`lib/playfair`),
which took it from [EstebanKubata/playfair](https://github.com/EstebanKubata/playfair).
GNU GPL v3 (see `LICENSE.md`), which section 13 of the AGPL lets UwUMirror
combine with. Built by `../build.rs` with the `cc` crate; the only entry point
UwUMirror uses is `playfair_decrypt` (see `src/fairplay.rs`).
