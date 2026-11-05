export const CHART_PAINTS = {
  folder: { fill: '#3272d1', ink: '#ffffff' },
  image: { fill: '#e8aa35', ink: '#24210c' },
  video: { fill: '#e36a4a', ink: '#241813' },
  audio: { fill: '#b76ac5', ink: '#201528' },
  code: { fill: '#219b8c', ink: '#05241f' },
  archive: { fill: '#7a63c6', ink: '#ffffff' },
  document: { fill: '#4aa3d5', ink: '#0c2736' },
  neutral: { fill: '#8a96a8', ink: '#142130' },
  unknown: { fill: '#616e81', ink: '#ffffff' },
} as const;

export const RANKED_PAINTS = [
  CHART_PAINTS.folder,
  CHART_PAINTS.archive,
  CHART_PAINTS.code,
  CHART_PAINTS.video,
  CHART_PAINTS.image,
  CHART_PAINTS.audio,
  CHART_PAINTS.document,
  CHART_PAINTS.neutral,
] as const;

export const SIZE_PAINTS = [
  { fill: '#55bfd5', ink: '#172232' },
  { fill: '#5998e8', ink: '#172232' },
  { fill: '#a28ce3', ink: '#172232' },
  { fill: '#e6b74a', ink: '#172232' },
  { fill: '#ed8549', ink: '#172232' },
] as const;

export const AGE_PAINTS = [
  { fill: '#39b4a0', ink: '#102820' },
  { fill: '#8fc56a', ink: '#202a16' },
  { fill: '#e6bd53', ink: '#302813' },
  { fill: '#e99457', ink: '#322317' },
  { fill: '#9a79d0', ink: '#21172d' },
] as const;
