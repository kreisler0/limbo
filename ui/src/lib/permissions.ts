import type { IconName } from './design/icons';

const LABELS: Record<string, string> = {
  camera: 'Camera',
  microphone: 'Microphone',
  location: 'Location',
  notifications: 'Notifications',
  sensors: 'Motion sensors',
  clipboard: 'Clipboard',
  multipleDownloads: 'Automatic downloads',
  files: 'Files',
  fonts: 'Local fonts',
  midi: 'MIDI devices',
  windowManagement: 'Window management',
};

const ASKS: Record<string, string> = {
  camera: 'use your camera',
  microphone: 'use your microphone',
  location: 'know your location',
  notifications: 'show notifications',
  sensors: 'use motion sensors',
  clipboard: 'see text and images you copy',
  multipleDownloads: 'download multiple files',
  files: 'edit files on your device',
  fonts: 'use fonts installed on your device',
  midi: 'control MIDI devices',
  windowManagement: 'manage windows on your displays',
};

export function permissionLabel(kind: string): string {
  return LABELS[kind] ?? kind;
}

export function permissionAsk(kind: string): string {
  return ASKS[kind] ?? `use ${kind}`;
}

export function permissionIcon(kind: string): IconName {
  switch (kind) {
    case 'camera':
      return 'camera';
    case 'microphone':
      return 'mic';
    case 'location':
      return 'mapPin';
    case 'notifications':
      return 'bell';
    case 'clipboard':
      return 'clipboard';
    case 'multipleDownloads':
      return 'download';
    case 'files':
      return 'folder';
    default:
      return 'shield';
  }
}
