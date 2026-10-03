#!/usr/bin/env python3
"""Render the reviewable tool readiness matrix from the authoritative JSON catalog."""

import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOG = ROOT / "catalog" / "tools.json"
OUTPUT = ROOT / "docs" / "tool-matrix.md"

TEST_EVIDENCE = {
    "arcade.text.case": "`arcade-core/src/text_tools/clean.rs`: `cases_cover_word_boundaries_and_unicode`",
    "arcade.text.clean": "`arcade-core/src/text_tools/clean.rs`: `cleanup_steps_combine_in_order`",
    "arcade.text.structured": "`arcade-core/src/text_tools/structured.rs`: `keys_and_elements_keep_their_order`, `toml_dates_nulls_and_detection`, `xml_output_is_indented_and_round_trips_attributes`",
    "arcade.text.diff": "`arcade-core/src/text_tools.rs`: `two_way_diff_supports_side_by_side_and_inline_views`, `three_way_merge_combines_separate_changes_and_marks_conflicts`",
    "arcade.text.csv": "`arcade-core/src/text_tools.rs`: `delimiter_detection_respects_quoted_commas`, `json_table_can_be_renamed_and_exported_as_csv`",
    "arcade.video.inspect": "`arcade-core/src/media.rs`: `installed_ffmpeg_runs_basic_video_workflows`",
    "arcade.video.convert": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool`, `plan_rejects_codecs_the_container_cannot_hold`",
    "arcade.video.trim": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (copy, keyframe snap, precise)",
    "arcade.video.crop": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (aspect, drawn area, bounds)",
    "arcade.video.compress": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (quality, two-pass target, resolution, batch)",
    "arcade.video.join": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (concat copy and mixed-clip transcode)",
    "arcade.video.extract-audio": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (copy table and FLAC fallback)",
    "arcade.video.gif": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (GIF palette, WebP when available)",
    "arcade.video.frames": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_runs_every_video_tool` (timestamp, interval, every Nth, limit)",
    "arcade.video.subtitles": "`arcade-core/src/media/video.rs`: `installed_ffmpeg_handles_subtitle_tracks` (bitmap burn untested)",
    "arcade.video.auto-subtitles": "`arcade-core/src/media/speech.rs`: `subtitles_split_long_segments_and_format_times` (Groq call verified manually)",
    "arcade.audio.transcribe": "`arcade-core/src/media/speech.rs`: `subtitles_split_long_segments_and_format_times` (Groq call verified manually)",
    "arcade.audio.convert": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool` (tags, cover, batch, bitrate limits)",
    "arcade.audio.trim": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool` (copy, fades, cut out middle)",
    "arcade.audio.join": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool` (copy, mixed formats, crossfade, gap)",
    "arcade.audio.normalize": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool` (two-pass loudness measured on output, peak, batch)",
    "arcade.audio.speed-pitch": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool` (tempo, resample, pitch shift)",
    "arcade.audio.silence": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool`, `silence_spans_and_audible_parts_are_complementary`",
    "arcade.audio.metadata": "`arcade-core/src/media/audio.rs`: `installed_ffmpeg_runs_every_audio_tool` (inspect, edit, cover add/remove, Ogg comments)",
    "arcade.pdf.merge": "`arcade-core/src/pdf.rs`: `installed_qpdf_merges_selected_pages_without_replacing_sources`",
    "arcade.pdf.split": "`arcade-core/src/pdf.rs`: `installed_qpdf_extracts_ranges_and_splits_every_n_pages_safely`",
    "arcade.pdf.organize": "`arcade-core/src/pdf.rs`: `organizer_per_page_rotation_rules_validate_output_page_ranges`",
    "arcade.pdf.compress": "`arcade-core/src/pdf.rs`: `installed_qpdf_lossless_optimize_preserves_pages_and_source`, `pdf_optimization_modes_set_explicit_quality_and_reject_bad_values`",
    "arcade.pdf.fill": "`arcade-core/src/pdf/form.rs`: `fields_are_listed_and_filled`, `values_parse_from_lines_pairs_and_json`",
    "arcade.pdf.pdf-to-images": "`arcade-core/src/pdf.rs`: `installed_poppler_extracts_metadata_and_renders_pdf_pages`",
    "arcade.pdf.extract": "`arcade-core/src/pdf.rs`: `installed_poppler_extracts_embedded_images_as_granted_files`, `installed_poppler_extracts_attachments_to_generated_non_overwriting_files`",
    "arcade.image.convert": "`arcade-core/src/image.rs`: `installed_vips_converts_and_resizes_real_images`",
    "arcade.image.resize": "`arcade-core/src/image.rs`: `installed_vips_converts_and_resizes_real_images`, `size_modes_bound_output`",
    "arcade.image.crop": "`arcade-core/src/image.rs`: `installed_vips_edits_redacts_compares_and_extracts_palette`",
    "arcade.image.redact": "`arcade-core/src/image.rs`: `installed_vips_edits_redacts_compares_and_extracts_palette`, `image/pii.rs`: `line_level_matching_finds_split_numbers_and_hides_text`",
    "arcade.image.compare": "`arcade-core/src/image.rs`: `installed_vips_edits_redacts_compares_and_extracts_palette`",
    "arcade.image.palette": "`arcade-core/src/image.rs`: `installed_vips_edits_redacts_compares_and_extracts_palette`",
    "arcade.image.metadata": "`arcade-core/src/image.rs`: `metadata_sanitizer_removes_embedded_exif_after_applying_orientation`",
    "arcade.image.passport": "`arcade-core/src/image/compose.rs`: `passport_presets_map_to_300_dpi_pixels`",
    "arcade.image.watermark": "`arcade-core/src/magick.rs`: `overlay_text_cannot_read_files_or_expand_escapes`",
    "arcade.web.transcript": "`arcade-core/src/web/ytdlp.rs`: `rolling_auto_captions_are_deduplicated`",
    "arcade.web.images": "`arcade-core/src/web/page_images.rs`: `finds_images_with_best_srcset_and_metadata`",
    "arcade.web.file-downloader": "`arcade-core/src/web.rs`: `downloader_resume_requires_strong_etag_and_exact_range_metadata`, `content_disposition_and_url_filenames_are_sanitized`",
    "arcade.web.markdown": "`arcade-core/src/web.rs`: `html_extractor_skips_active_content_and_rejects_script_urls`",
    "arcade.network.site-check": "`arcade-core/src/network.rs`: `rejects_unsafe_url_protocols_credentials_and_invalid_hosts`",
    "arcade.convert.calculator": "`arcade-core/src/converter_tools/calculator.rs`: `calculator_parses_functions_and_precedence`",
    "arcade.convert.percentage": "`arcade-core/src/converter_tools/finance.rs`: `percentage_modes_cover_everyday_cases`",
    "arcade.convert.loan": "`arcade-core/src/converter_tools/finance.rs`: `loan_emi_matches_the_standard_formula`",
    "arcade.convert.units": "`arcade-core/src/converter_tools/units.rs`: `unit_conversions_cover_temperature_and_data`",
    "arcade.convert.date-duration": "`arcade-core/src/converter_tools/datetime.rs`: `differences_use_calendar_months_and_weekdays`, `timestamps_convert_both_ways`",
    "arcade.convert.time-zone": "`arcade-core/src/converter_tools/datetime.rs`: `nonexistent_local_times_are_rejected`",
    "arcade.convert.color": "`arcade-core/src/converter_tools/color.rs`: `formats_round_trip_and_contrast_matches_wcag`",
    "arcade.convert.excel-csv": "`arcade-core/src/converter_tools/spreadsheet.rs`: `number_detection_keeps_codes_as_text`, `sheet_names_are_valid_and_unique`",
    "arcade.utility.emoji": "`arcade-core/src/utility_tools.rs`: `emoji_search_ranks_name_matches_first`",
    "arcade.utility.random": "`arcade-core/src/utility_tools.rs`: `random_modes_respect_limits`",
    "arcade.barcode.qr-generate": "`arcade-core/src/barcode.rs`: `generated_qr_round_trips_through_the_decoder`",
    "arcade.barcode.decode": "`arcade-core/src/barcode.rs`: `generated_qr_round_trips_through_the_decoder`",
    "arcade.developer.encoding": "`arcade-core/src/developer_tools.rs`: `encoding_round_trips_common_formats`",
    "arcade.developer.url": "`arcade-core/src/developer_tools.rs`: `url_encoding_is_component_safe_and_decodes_utf8`",
    "arcade.developer.identifier": "`arcade-core/src/developer_tools.rs`: `ulid_has_canonical_width_and_alphabet`",
    "arcade.developer.cron": "`arcade-core/src/developer_tools.rs`: `cron_parser_handles_lists_steps_and_sunday_alias`",
    "arcade.files.rename": "`arcade-core/src/file_tools.rs`: `rename_compare_and_size_tools_return_reviewable_results`",
    "arcade.files.duplicates": "`arcade-core/src/file_tools.rs`: `duplicate_finder_stays_within_granted_root`",
    "arcade.files.compare": "`arcade-core/src/file_tools.rs`: `rename_compare_and_size_tools_return_reviewable_results`",
    "arcade.files.sizes": "`arcade-core/src/file_tools.rs`: `rename_compare_and_size_tools_return_reviewable_results`",
    "arcade.security.password": "`arcade-core/src/security_tools.rs`: `pseudo_word_password_has_configured_number_of_tokens`",
    "arcade.security.encrypt": "`arcade-core/src/security_tools.rs`: `age_file_encrypt_decrypt_round_trip_runs_through_registry`, `age_recipient_encrypt_decrypt_round_trip_runs_through_registry`",
}


def cell(value: str) -> str:
    return value.replace("|", "&#124;").replace("\n", " ")


def main() -> None:
    catalog = json.loads(CATALOG.read_text(encoding="utf-8"))
    lines = [
        "# Tool readiness matrix",
        "",
        "Generated from [`catalog/tools.json`](../catalog/tools.json) by `python3 catalog/render_tool_matrix.py`. Edit the catalog, then regenerate this view. `planned` and `partial` are not release-ready. Test evidence is a direct reference when known; all other entries remain pending.",
        "",
        "| ID | Status | Category | Provider capabilities | Privacy | Windows | macOS | Wayland | X11 | Pipeline inputs → outputs | Tests |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for tool in catalog["tools"]:
        platform = tool["platforms"]
        types = ", ".join(tool["inputs"]) + " → " + ", ".join(tool["outputs"])
        tests = TEST_EVIDENCE.get(tool["id"], "Pending")
        fields = [
            tool["id"],
            tool["status"],
            tool["category"],
            ", ".join(tool["providers"]) or "—",
            tool["privacyClass"],
            platform.get("windows", "unknown"),
            platform.get("macos", "unknown"),
            platform.get("wayland", "unknown"),
            platform.get("x11", "unknown"),
            types,
            tests,
        ]
        lines.append("| " + " | ".join(cell(str(v)) for v in fields) + " |")
    rendered = "\n".join(lines) + "\n"
    if "--check" in sys.argv:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != rendered:
            raise SystemExit("Tool matrix is stale; run python3 catalog/render_tool_matrix.py")
    else:
        OUTPUT.write_text(rendered, encoding="utf-8")


if __name__ == "__main__":
    main()
