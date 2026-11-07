export type HealthUnavailable =
  | 'recorded_scan'
  | 'root_changed'
  | 'volume_unmapped'
  | 'extent_limit'
  | 'mapping_changed'
  | 'access_denied'
  | 'unsupported'
  | 'invalid_response'
  | 'driver_error';

type Unavailable = { kind: 'unavailable'; reason: HealthUnavailable };

export type DiskHealth =
  | {
      kind: 'nvme';
      critical_warning: number;
      temperature_kelvin: number | null;
      power_on_hours: string;
      unsafe_shutdowns: string;
      media_errors: string;
    }
  | { kind: 'windows_prediction'; alert: boolean }
  | Unavailable;

export type DeviceHealthReport = {
  root: string;
  checked_at: number | null;
  observation: { kind: 'disks'; items: { number: number; health: DiskHealth }[] } | Unavailable;
};
