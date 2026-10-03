<script lang="ts">
  import type { IconName } from './icon-names';

  export let name: IconName;
  export let size = 18;
  export let stroke = 1.8;
</script>

<svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={stroke} stroke-linecap="round" stroke-linejoin="round">
  {#if name === 'spark'}
    <path d="m12 3 1.6 5.4L19 10l-5.4 1.6L12 17l-1.6-5.4L5 10l5.4-1.6L12 3Z"/><path d="m19 15 .8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8L19 15Z"/>
  {:else if name === 'search'}
    <circle cx="10.8" cy="10.8" r="6.5"/><path d="m16 16 4.2 4.2"/>
  {:else if name === 'grid'}
    <rect x="3.5" y="3.5" width="7" height="7" rx="2"/><rect x="13.5" y="3.5" width="7" height="7" rx="2"/><rect x="3.5" y="13.5" width="7" height="7" rx="2"/><rect x="13.5" y="13.5" width="7" height="7" rx="2"/>
  {:else if name === 'arrow'}
    <path d="M5 12h14M13 6l6 6-6 6"/>
  {:else if name === 'close'}
    <path d="m6 6 12 12M18 6 6 18"/>
  {:else if name === 'download'}
    <path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 19h14"/>
  {:else if name === 'qr'}
    <rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><path d="M14 14h2v2h-2zM18 18h2v2h-2zM14 18v2M18 14h2"/>
  {:else if name === 'monitor'}
    <rect x="3" y="4.5" width="18" height="12" rx="2"/><path d="M9 20h6M12 16.5V20"/>
  {:else if name === 'refresh'}
    <path d="M20 11a8 8 0 0 0-14.9-3L3 11m0 0V5m0 6h6M4 13a8 8 0 0 0 14.9 3L21 13m0 0v6m0-6h-6"/>
  {:else if name === 'chevron'}
    <path d="m9 18 6-6-6-6"/>
  {:else if name === 'clock'}
    <circle cx="12" cy="12" r="8.5"/><path d="M12 7v5l3.2 2"/>
  {:else if name === 'star'}
    <path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9L12 3Z"/>
  {:else if name === 'copy'}
    <rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5.5A1.5 1.5 0 0 0 14.5 4h-9A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16H8"/>
  {:else if name === 'check'}
    <path d="m5 12.5 4.2 4.2L19 7"/>
  {:else if name === 'play'}
    <path d="m8 5 11 7-11 7V5Z"/>
  {:else if name === 'pause'}
    <path d="M8 5v14M16 5v14"/>
  {:else if name === 'back'}
    <path d="m15 18-6-6 6-6M9 12h11"/>
  {:else if name === 'file'}
    <path d="M13 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V10Z"/><path d="M13 3v7h7M8 15h8M8 18h5"/>
  {:else if name === 'lock'}
    <rect x="4" y="10" width="16" height="11" rx="2.5"/><path d="M8 10V7a4 4 0 1 1 8 0v3m-4 5v2"/>
  {:else if name === 'external'}
    <path d="M14 4h6v6M20 4l-9 9"/><path d="M18 13v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h5"/>
  {:else if name === 'dots'}
    <circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>
  {:else if name === 'plus'}
    <path d="M12 5v14M5 12h14"/>
  {:else if name === 'command'}
    <path d="M9 6a2 2 0 1 1-2-2h2v5H6a2 2 0 1 1 0 4h3v5H7a2 2 0 1 1 2-2v-3h6v3a2 2 0 1 1 2 2h-2v-5h3a2 2 0 1 1 0-4h-3V4h2a2 2 0 1 1-2 2v3H9V6Z"/>
  {:else if name === 'moon'}
    <path d="M20 15.2A8.4 8.4 0 0 1 8.8 4 8.4 8.4 0 1 0 20 15.2Z"/>
  {:else if name === 'sun'}
    <circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>
  {:else if name === 'globe' || name === 'network'}
    <circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a15 15 0 0 1 0 18M12 3a15 15 0 0 0 0 18"/>
  {:else if name === 'code'}
    <path d="m8 8-4 4 4 4m8-8 4 4-4 4m-2-11-4 14"/>
  {:else if name === 'image'}
    <rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="8.5" cy="9" r="1.5"/><path d="m21 15-5-5L5 20"/>
  {:else if name === 'video'}
    <rect x="3" y="5" width="13" height="14" rx="3"/><path d="m16 10 5-3v10l-5-3z"/>
  {:else if name === 'audio'}
    <path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>
  {:else if name === 'document'}
    <path d="M13 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V10Z"/><path d="M13 3v7h7M8 14h8m-8 4h8"/>
  {:else if name === 'folder'}
    <path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"/>
  {:else if name === 'trash'}
    <path d="M4 7h16M9 7V4h6v3m3 0-1 13H7L6 7m3 4v5m6-5v5"/>
  {:else if name === 'pin'}
    <path d="m16 3 5 5-4 1-3 3-1 5-3-3-7 7m3-12 5-3 1-4"/>
  {:else if name === 'clipboard'}
    <rect x="5" y="5" width="14" height="16" rx="2"/><path d="M9 5.5V4h6v1.5M8 10h8m-8 4h8m-8 4h5"/>
  {:else if name === 'shield'}
    <path d="M12 3 20 6v5c0 5-3.4 8.2-8 10-4.6-1.8-8-5-8-10V6l8-3Z"/><path d="m9 12 2 2 4-4"/>
  {:else if name === 'calculator'}
    <rect x="5" y="3" width="14" height="18" rx="2.5"/><path d="M8 7h8m-8 5h.01M12 12h.01M16 12h.01M8 16h.01M12 16h.01M16 16h.01"/>
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3"/><path d="m19.4 15 .1.1a1.7 1.7 0 0 1-2.4 2.4l-.1-.1a1.7 1.7 0 0 0-2.9 1.2v.2a1.7 1.7 0 0 1-3.4 0v-.2a1.7 1.7 0 0 0-2.9-1.2l-.1.1a1.7 1.7 0 0 1-2.4-2.4l.1-.1a1.7 1.7 0 0 0-1.2-2.9H4a1.7 1.7 0 0 1 0-3.4h.2a1.7 1.7 0 0 0 1.2-2.9l-.1-.1a1.7 1.7 0 0 1 2.4-2.4l.1.1a1.7 1.7 0 0 0 2.9-1.2V2a1.7 1.7 0 0 1 3.4 0v.2a1.7 1.7 0 0 0 2.9 1.2l.1-.1a1.7 1.7 0 0 1 2.4 2.4l-.1.1a1.7 1.7 0 0 0 1.2 2.9h.2a1.7 1.7 0 0 1 0 3.4h-.2a1.7 1.7 0 0 0-1.2 2.9Z" transform="translate(1 2) scale(.92)"/>
  {:else}
    <path d="M5 5h14M5 12h14M5 19h14M8 3v4m8 3v4m-8 3v4"/>
  {/if}
</svg>
