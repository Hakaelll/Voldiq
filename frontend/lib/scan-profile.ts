export type ScanProfile = 'auto' | 'eco' | 'balanced' | 'fast';

export const scanProfiles: readonly { id: ScanProfile; label: string; description: string }[] = [
  {
    id: 'auto',
    label: 'Automatic',
    description: 'Adapts metadata concurrency to storage. Unknown storage uses one worker.',
  },
  {
    id: 'eco',
    label: 'Eco',
    description: 'One metadata worker with short pauses to reduce disk activity.',
  },
  { id: 'balanced', label: 'Balanced', description: 'Up to two metadata workers.' },
  {
    id: 'fast',
    label: 'Fast',
    description: 'Up to eight metadata workers. May increase disk activity.',
  },
];

export function parseScanProfile(value: unknown): ScanProfile | null {
  switch (value) {
    case 'auto':
    case 'eco':
    case 'balanced':
    case 'fast':
      return value;
    default:
      return null;
  }
}
