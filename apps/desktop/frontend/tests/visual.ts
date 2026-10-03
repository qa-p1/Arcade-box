// Development-only visual fixture. Vite does not include this entry in production.
// Supplies catalog/settings and explicitly injected results for UI regression checks.
// This never exercises native execution; native commands have separate Rust tests.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { mount } from 'svelte';
import { emit } from '@tauri-apps/api/event';
import App from '../src/App.svelte';
import catalog from '../../../../catalog/tools.json';
import '../src/app.css';

const fixture = {
  commands: [] as Array<{ command: string; args: unknown }>,
  result: null as unknown,
  imagePreview: null as unknown,
  files: [] as unknown[],
  providers: [] as unknown[],
};

// Painted stand-ins for FFmpeg stills so the video editors can be checked visually.
function fixtureFrame(index: number, height: number): string {
  const canvas = document.createElement('canvas');
  canvas.width = Math.round(height * 16 / 9);
  canvas.height = height;
  const context = canvas.getContext('2d')!;
  const gradient = context.createLinearGradient(0, 0, canvas.width, canvas.height);
  gradient.addColorStop(0, `hsl(${200 + index * 14} 45% 32%)`);
  gradient.addColorStop(1, `hsl(${250 + index * 14} 40% 18%)`);
  context.fillStyle = gradient;
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = 'rgba(255,255,255,.18)';
  context.beginPath();
  context.arc(canvas.width * (0.3 + index * 0.04), canvas.height * 0.55, height * 0.22, 0, Math.PI * 2);
  context.fill();
  return canvas.toDataURL('image/jpeg', 0.7);
}

// A spoken-word-like envelope: phrases separated by short pauses.
function audioPreviewFixture(columns: number, silence: boolean) {
  const duration = 212.4;
  const peaks = Array.from({ length: columns }, (_, index) => {
    const pause = index % 37 > 31 || index < 3 || index > columns - 6;
    return pause ? 0.002 : 0.25 + 0.6 * Math.abs(Math.sin(index * 0.7) * Math.cos(index * 0.13));
  });
  const silences: Array<[number, number]> = [];
  if (silence) {
    silences.push([0, 2.6]);
    for (let column = 32; column < columns; column += 37) silences.push([(column / columns) * duration, ((column + 5) / columns) * duration]);
    silences.push([duration - 4.4, duration]);
  }
  return {
    durationSeconds: duration, codec: 'mp3', sampleRate: 44100, channels: 2, channelLayout: 'stereo', bitRate: 192000,
    sourceBytes: 5_100_000, container: 'mp3', tracks: 1, lossless: false, copyable: true, language: null,
    tags: { title: 'Episode 12 — Night Shift', artist: 'Arcade Radio', album: 'Season Two', date: '2024', genre: 'Podcast' },
    cover: fixtureFrame(5, 120), peaks, silences: silence ? silences : null,
  };
}

function videoPreviewFixture(thumbnails: number, frameAt: number | null) {
  const duration = 84.5;
  return {
    durationSeconds: duration, width: 1920, height: 1080, frameRate: 29.97, videoCodec: 'h264', sourceBytes: 48_200_000,
    audio: [
      { index: 0, kind: 'audio', codec: 'aac', language: 'eng', title: 'Stereo', detail: 'AAC · stereo · 48 kHz', bitmap: false },
      { index: 1, kind: 'audio', codec: 'ac3', language: 'jpn', title: null, detail: 'AC-3 · 5.1 · 48 kHz', bitmap: false },
    ],
    subtitles: [
      { index: 0, kind: 'subtitle', codec: 'subrip', language: 'eng', title: null, detail: 'SubRip text', bitmap: false },
      { index: 1, kind: 'subtitle', codec: 'hdmv_pgs_subtitle', language: 'eng', title: 'Signs', detail: 'PGS image', bitmap: true },
    ],
    thumbnails: Array.from({ length: thumbnails }, (_, index) => ({ timeSeconds: duration * (index + 0.5) / thumbnails, dataUrl: fixtureFrame(index, 72) })),
    frame: frameAt === null ? null : { timeSeconds: frameAt < 0 ? duration / 2 : frameAt, dataUrl: fixtureFrame(3, 540) },
  };
}

Object.assign(window, { __arcadeFixture: fixture });
mockWindows('island');
mockIPC((command, args) => {
  if (!['set_island_input_region', 'plugin:event|listen'].includes(command)) fixture.commands.push({ command, args });
  switch (command) {
    case 'start_job': {
      if (!fixture.result) throw new Error('Inject a test result before running a visual fixture.');
      return { id: 'visual-job', toolId: (args?.request as { toolId: string }).toolId, status: 'succeeded', progress: 1, result: fixture.result };
    }
    case 'copy_text': case 'copy_image_result': return;
    case 'image_result_preview': {
      if (!fixture.imagePreview) throw new Error('No image preview fixture supplied.');
      return fixture.imagePreview;
    }
    case 'select_files': return fixture.files;
    case 'audio_preview': return audioPreviewFixture(Number(args?.columns ?? 0), args?.silenceThresholdDb != null);
    case 'measure_audio_loudness': return { integrated: -23.4, truePeak: -4.1, range: 7.2, threshold: -33.6 };
    case 'video_preview': return videoPreviewFixture(Number(args?.thumbnails ?? 0), (args?.frameAt as number | null) ?? null);
    case 'estimate_video_output': return { estimatedBytes: 21_400_000, sourceBytes: 48_200_000, durationSeconds: 84.5, method: 'sample', sampledSeconds: 12, warnings: [] };
    case 'list_tools': return catalog.tools;
    case 'search_tools': {
      const query = String(args?.query ?? '').toLowerCase();
      return catalog.tools.filter((tool) => [tool.name, tool.description, ...tool.aliases].join(' ').toLowerCase().includes(query)).slice(0, 12);
    }
    case 'get_preference': return args?.key === 'theme' ? 'dark' : 'true';
    case 'shortcut_status': return { backend: 'visual-fixture', state: 'registered', triggerDescription: 'Ctrl+Alt+Space', message: 'Visual fixture only' };
    case 'list_providers': return fixture.providers;
    case 'list_jobs': case 'get_history': case 'list_favorites':
    case 'detect_context': case 'list_pipelines': case 'list_plugins': return [];
    case 'screen_capture_status': return { captureAvailable: false, recordingAvailable: false, message: 'Use the native app for capture.' };
    case 'screen_recording_status': return { recording: false };
    case 'paste_plain_status': case 'window_pin_status': return { available: false };
    case 'island_ready': void emit('arcade://island-shown'); return;
    case 'set_island_input_region': return;
    case 'set_surface_mode': case 'set_preference': case 'set_favorite': case 'hide_island': return;
    case 'plugin:event|listen': return 1;
    case 'plugin:event|unlisten': return;
    case 'plugin:window|is_focused': return true;
    default: throw new Error(`Native integration required: ${command}`);
  }
}, { shouldMockEvents: true });
mount(App, { target: document.getElementById('app')! });
