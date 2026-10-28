# PDF Protect / Unlock

**LOCAL** · Requires system `qpdf` 11.9 or newer.

Protect creates a new PDF using 256-bit AES with an open password; printing and modification permissions can be selected. The owner password, which is needed to change those limits later, is optional. When it is left empty, a random 40-character one is generated and not shown, so the limits can't be lifted. Unlock removes encryption only when the password you supply is valid. Arcade Box does not attempt password recovery.

Passwords use masked fields, are not saved in pipeline definitions, and are passed to qpdf via a private temporary argument file. PDF permission restrictions depend on reader support and do not replace file-level access control.
