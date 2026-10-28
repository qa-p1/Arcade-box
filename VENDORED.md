# Vendored Arcade Link assets

Shared installer, release, shortcut and protocol assets are copied unchanged
from [Arcade Link](https://github.com/qa-p1/Arcade-Link/tree/v0.4.0) tag `v0.4.0`
(commit `498b47f17aa35864aef765ac56a877d06b927dbe`). License: MIT OR Apache-2.0.
Run `python3 scripts/check-vendored.py` to verify the SHA-256 inventory.
Refresh the source pin and hashes together.

| App path | Link source |
| --- | --- |
| `assets/strings/install.json` | `assets/strings/install.json` |
| `packaging/windows/README.md` | `packaging/windows/README.md` |
| `packaging/windows/arcade-integration.ps1` | `packaging/windows/arcade-integration.ps1` |
| `packaging/windows/arcade.iss` | `packaging/windows/arcade.iss` |
| `packaging/windows/arcade.nsh` | `packaging/windows/arcade.nsh` |
| `scripts/arcade-release.py` | `tools/arcade-release.py` |
| `scripts/test_arcade_release.py` | `tools/test_arcade_release.py` |
| `scripts/test_validate_shortcuts.py` | `tools/test_validate_shortcuts.py` |
| `scripts/validate_shortcuts.py` | `tools/validate_shortcuts.py` |
| `spec/arcade-release.schema.json` | `spec/arcade-release.schema.json` |
| `spec/receipt.schema.json` | `spec/receipt.schema.json` |
| `spec/shortcuts.schema.json` | `spec/shortcuts.schema.json` |
| `spec/vectors/accelerators.json` | `spec/vectors/accelerators.json` |
| `spec/vectors/methods.json` | `spec/vectors/methods.json` |
| `spec/vectors/receipts.json` | `spec/vectors/receipts.json` |
| `spec/vectors/shortcuts.json` | `spec/vectors/shortcuts.json` |
| `spec/vectors/trayhost.json` | `spec/vectors/trayhost.json` |

```text
3b5d2981ab87fdf07437daa294dce825d59b63580430e9ffa2b58e87d5d72cf0  tools/validate_shortcuts.py
d1f3ac5e6a854a5a46c03eb53600be4c25f698e90cdd818f2aaf3b600bb89398  spec/shortcut-sheet.schema.json
0c829174d2dd5e15930cd2c4c5cb791ab6f744a3dab13ed557161aabcacf5fce  assets/strings/install.json
11e3deb7969db46ff7b7cc6afc1a0e39b81fa379f96fced2f56624a60512616f  packaging/windows/README.md
2b2a8b699dc1e6c81c02bb9ad9be1ddf2f8a5447541eb1dfe886056aa5dc4fd6  packaging/windows/arcade-integration.ps1
fe152028ff674fdacdef4aed8970bb0a7afeb2dd67d2a3af075d7d95683bf96b  packaging/windows/arcade.iss
1f548b420bc08b479d6c65801a208aa67dc80fef851b5e2a49696d307562831e  packaging/windows/arcade.nsh
8f360855419930730c486d0978acaecf7a8179cc063465c9f8324878dcf6c2fd  scripts/arcade-release.py
58d78ac6a7b5d129bac4509e59f50fedb9db8c1fd2247431726bc8768776439a  scripts/test_arcade_release.py
8e616c4585084906a4976b207b4cb657c26ab05388e78fc3cdd79df942689dae  scripts/test_validate_shortcuts.py
3b5d2981ab87fdf07437daa294dce825d59b63580430e9ffa2b58e87d5d72cf0  scripts/validate_shortcuts.py
b3f4fffd8929a6a9449947b303b9e07171de8a5820d576f2d47b002944a00624  spec/arcade-release.schema.json
bf510b32071cd2883cee1e258afbd3bcf51fd6eb92f07e8a58cc117686546e07  spec/receipt.schema.json
68e8ed4c5adae97895b215813f5a528b53f2b720bb428112e94ab35489269a1a  spec/shortcuts.schema.json
67d0d5a86e8d9f953c2cf779136c790bdd62d7983fa12d36a65c446e12b246ec  spec/vectors/accelerators.json
a3c5796e067e4e0d71dad453e1be141a873b0999cba6fe9616c866be0906e2e5  spec/vectors/methods.json
aebde82ca97adebe6c4fdd5d0dea338d1134db5ccddf0678ec4cb6d836f94198  spec/vectors/receipts.json
2691d5b60314b7060bac5c219ada25edbfdc30829ea8a22d7496714afe2d4cc5  spec/vectors/shortcuts.json
4e1ce6155a2fbff13cfb5878ed66ee691843ecc36b631693914d5d893b2e8204  spec/vectors/trayhost.json
```

The custom NSIS template is based on Tauri `tauri-cli-v2.12.1` and retains the adjacent MIT and Apache-2.0 license files; its Arcade adaptations are local code.

Tauri template source: `tauri-cli-v2.12.1`, upstream SHA-256
`dabed59013b1d78b879a1a85bc7f2eed2993b33a9a90cdabe5946de3d3950597`.
The template contains local Arcade adaptations. SHA-256 inventory:

```text
3ffc4150b03dadadbf8c6469723269aa36204a1f7c326f5045c0daf4b6a45500  apps/desktop/src-tauri/windows/installer.nsi
9dd42ea92cff2ede5cd477cbfcce051b2d0115c0ac7f368ee88cb545055dff1d  apps/desktop/src-tauri/windows/LICENSE-TAURI-MIT
0d542e0c8804e39aa7f37eb00da5a762149dc682d7829451287e11b938e94594  apps/desktop/src-tauri/windows/LICENSE-TAURI-APACHE-2.0
```

## Arcade design kit

Copied unchanged from [Arcade Link](https://github.com/qa-p1/Arcade-Link/tree/v0.4.0) tag `v0.4.0` (commit
`498b47f17aa35864aef765ac56a877d06b927dbe`) by `scripts/sync-design.py`; never edit these files here.
License: MIT OR Apache-2.0; Inter is under the SIL Open Font License 1.1
(`apps/desktop/frontend/src/design/fonts/OFL.txt`).

```text
9d7c91751db00791c4625dfc2de92cee49d1604dfea9e4ef1c84e9d232953b10  apps/desktop/frontend/src/design/arcade.css
69a7a24cd12ce0981eab0a10145cc1f18afc9829d6351fb8673ded33147c88ab  apps/desktop/frontend/src/design/icons.ts
9f30255bf79330c12e962502f92059df256c6360f9df844c334f912aff85b9d8  apps/desktop/frontend/src/design/format.ts
693b77d4f32ee9b8bfc995589b5fad5e99adf2832738661f5402f9978429a8e3  apps/desktop/frontend/src/design/fonts/InterVariable.woff2
262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a  apps/desktop/frontend/src/design/fonts/OFL.txt
11405845a97dcec4150fb3943d03dbd11eeb086ded31a5cab27aa92b620838c1  apps/desktop/frontend/src/design/glyphs/arcade.box.svg
bbf1518dc224aa8f8df59f432cf5eb8b6e25b1f4a4e4676fe231d84770dac156  apps/desktop/frontend/src/design/glyphs/arcade.clipboard.svg
54c0b3e869912313a7b543175ad6bda5853b0bc8118d753e2c67de6dfdd07df9  apps/desktop/frontend/src/design/glyphs/arcade.find.svg
ba869f44636f28ea28ebd71c8e00b8206fd1eaa28ce23cfc628b8feac1bb556b  apps/desktop/frontend/src/design/glyphs/arcade.lens.svg
ce15684dfd003b0beb7e21a6e8c5a080433a0e25b804e48f024f3ba17794279b  apps/desktop/frontend/src/design/glyphs/arcade.look.svg
42ff3fbca18365d18875061617446aca6876bf5f740160412663d3460f81e0cc  apps/desktop/frontend/src/design/glyphs/arcade.shelf.svg
dd2aa2c70bb8ebe8b597eb8440dfc032a548ff43f90f2ffd08ec00cd02b4fac2  apps/desktop/frontend/src/design/glyphs/arcade.tools.svg
6718ccee834c345aac4b4a71ea83461412e5f4ae6f24179ab897192cf243ce63  apps/desktop/frontend/src/design/glyphs/arcade.wheel.svg
cca5e74ae6c385c4a2b881b2c3588095657d6711b9a1999408ce309ae284d93b  apps/desktop/src-tauri/icons/128x128.png
9970c321dcaf162e5e0822fcb55fd4d4bf8e7cf661f2e81ef62a3f7273af50c0  apps/desktop/src-tauri/icons/128x128@2x.png
6b8709ce07d752b6162b225262a695430ebf538c055ca16cea25cc610c4c4774  apps/desktop/src-tauri/icons/32x32.png
325eb19e55c061b74e16676a39f43d5bc53ae81d80ffe3be30cb57869eef78b7  apps/desktop/src-tauri/icons/64x64.png
42c0cea2ad6ba137d4533f6ff54c8bd4a614bb9d147e09d73377b9f52d7ecb3d  apps/desktop/src-tauri/icons/Square107x107Logo.png
a67bb67c9d3a5ec02b7636eef6ad72f370fe0fc58d54dbf8d2dbf183b6c05c3d  apps/desktop/src-tauri/icons/Square142x142Logo.png
127c58e7802b45df61b11f4edd13231117a072c768e06ed5eec188a6ec92519c  apps/desktop/src-tauri/icons/Square150x150Logo.png
21f67405d0b3c627367158c8ceaf516bcd5b8a3ba43677a295dc677e7c6b2704  apps/desktop/src-tauri/icons/Square284x284Logo.png
3140d22de38380c28c3fc52f3e8f25c0aa56964881ad1c209c01dc558b46eb66  apps/desktop/src-tauri/icons/Square30x30Logo.png
64f3fdea8e8396b96c681fab75a168d5ed627e7bf0c06a1ab2a34a938a8f5049  apps/desktop/src-tauri/icons/Square310x310Logo.png
5343f2eb9f7f189db2acb0ae40aafe77617ce3c541be7a7376d6808667ee2468  apps/desktop/src-tauri/icons/Square44x44Logo.png
803026c2c861549397865937c7422dc217d3a3e13f33d1741ad704a5ae9007e4  apps/desktop/src-tauri/icons/Square71x71Logo.png
73d7fb9220b62feacd27e849335742f61c5f5f6dcf7d4313406bdedf70a85242  apps/desktop/src-tauri/icons/Square89x89Logo.png
e639ff49716e827646b649f148af9cef6d24834434635fc0c53bbb7ff232f984  apps/desktop/src-tauri/icons/StoreLogo.png
c30c8655791305ee1e09220d2aee7882767ef5051d4bc2f666cb0f80aac508ca  apps/desktop/src-tauri/icons/icon.icns
00cf87c3f3e8a6d24f224e9d4bf7a79d35c5c5c01dce22f441ff332c8e2f7000  apps/desktop/src-tauri/icons/icon.ico
a0f663858bd5a91deb8c350d6801bbafcb1f1a79cf43def5107115ae66bc2ef0  apps/desktop/src-tauri/icons/icon.png
afebfc174401274c785ece5a75dfc8fb4cc669fbaf3465ab11f5246bc898dbd1  apps/desktop/src-tauri/icons/icon.svg
```
