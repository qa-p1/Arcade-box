# Text Diff / Merge

Compares two texts in unified, side-by-side, or inline views, or makes a three-way merge of base, left, and right versions. Inputs may be supplied as selected text files, separate pipeline values, or pasted into the single text field using markers.

- For a two-way diff, separate the versions with a line containing `--- RIGHT ---`.
- For a three-way merge in one field, use `--- BASE ---`, `--- LEFT ---`, and `--- RIGHT ---` section markers in that order. Pipelines may instead supply exactly three inputs.
- Independent edits are combined. Identical edits are kept once. Overlapping edits that differ are preserved with `<<<<<<< LEFT`, `=======`, and `>>>>>>> RIGHT` markers and reported as conflicts.
- Combined input is limited to 16 MiB. Resolve all conflict markers before using the merged text.

The diff is generated locally and returns text. File inputs are read through the user’s selected-file grant; they are not modified.
