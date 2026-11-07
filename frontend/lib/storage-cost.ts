export type StorageUnit = 'GB' | 'GiB';
const unitBytes: Record<StorageUnit, bigint> = { GB: 1_000_000_000n, GiB: 1_073_741_824n };
const scale = 1_000_000n;

export type Usage = { readonly microbytes: bigint };

export type TariffDraft = {
  name: string;
  date: string;
  currency: string;
  unit: StorageUnit;
  model: 'metered' | 'fixed';
  price: string;
  capacity: string;
};

type Tariff = {
  name: string;
  date: string;
  currency: string;
  unit: StorageUnit;
  price: bigint;
  minorDigits: number;
  plan: { kind: 'metered' } | { kind: 'fixed'; capacity: Usage };
};

type ParseTariff = { kind: 'valid'; tariff: Tariff } | { kind: 'invalid'; message: string };

function decimalMicros(raw: string): bigint | null {
  const value = raw.trim().replace(',', '.');
  if (!/^(?:\d{1,12}(?:\.\d{1,6})?|\.\d{1,6})$/.test(value)) return null;
  const [whole, fraction = ''] = value.split('.');
  return BigInt(whole || '0') * scale + BigInt(fraction.padEnd(6, '0'));
}

export function enteredUsage(raw: string, unit: StorageUnit): Usage | null {
  const amount = decimalMicros(raw);
  return amount === null ? null : { microbytes: amount * unitBytes[unit] };
}

export function scannedUsage(bytes: number): Usage | null {
  return Number.isSafeInteger(bytes) && bytes >= 0 ? { microbytes: BigInt(bytes) * scale } : null;
}

export function parseTariff(draft: TariffDraft): ParseTariff {
  const name = draft.name.trim();
  if (!name || name.length > 48) return { kind: 'invalid', message: 'Enter a tariff name.' };
  const date = new Date(`${draft.date}T00:00:00Z`);
  if (
    !/^\d{4}-\d{2}-\d{2}$/.test(draft.date) ||
    !Number.isFinite(date.getTime()) ||
    date.toISOString().slice(0, 10) !== draft.date
  )
    return { kind: 'invalid', message: 'Enter a valid effective date.' };
  const currency = draft.currency.trim().toUpperCase();
  if (!Intl.supportedValuesOf('currency').includes(currency))
    return {
      kind: 'invalid',
      message: 'Enter a supported three-letter currency code, such as EUR.',
    };
  const price = decimalMicros(draft.price);
  if (price === null)
    return { kind: 'invalid', message: 'Enter a nonnegative price with up to six decimal places.' };

  let plan: Tariff['plan'] = { kind: 'metered' };
  if (draft.model === 'fixed') {
    const capacity = enteredUsage(draft.capacity, draft.unit);
    if (!capacity || capacity.microbytes === 0n)
      return { kind: 'invalid', message: 'Enter the included storage capacity above zero.' };
    plan = { kind: 'fixed', capacity };
  }
  const minorDigits = new Intl.NumberFormat('en', { style: 'currency', currency }).resolvedOptions()
    .maximumFractionDigits;
  return {
    kind: 'valid',
    tariff: {
      name,
      date: draft.date,
      currency,
      unit: draft.unit,
      price,
      minorDigits: minorDigits ?? 2,
      plan,
    },
  };
}

function decimalText(value: bigint, digits: number, trim: boolean): string {
  const denominator = 10n ** BigInt(digits);
  const whole = (value / denominator).toLocaleString('en');
  if (!digits) return whole;
  const fraction = (value % denominator).toString().padStart(digits, '0');
  const shown = trim ? fraction.replace(/0+$/, '') : fraction;
  return shown ? `${whole}.${shown}` : whole;
}

export function tariffLabel(tariff: Tariff): string {
  const price = `${tariff.currency} ${decimalText(tariff.price, 6, true)}`;
  if (tariff.plan.kind === 'metered') return `${price} / ${tariff.unit} / month`;
  const capacity = tariff.plan.capacity.microbytes / unitBytes[tariff.unit];
  return `${price} / month · ${decimalText(capacity, 6, true)} ${tariff.unit} included`;
}

export function monthlyEstimate(tariff: Tariff, usage: Usage | null): string {
  if (!usage) return 'Enter usage';
  const { plan, price, currency, minorDigits, unit } = tariff;
  if (plan.kind === 'fixed' && usage.microbytes > plan.capacity.microbytes)
    return 'Above included capacity';

  const numerator = plan.kind === 'fixed' ? price : price * usage.microbytes;
  const denominator = plan.kind === 'fixed' ? scale : scale * scale * unitBytes[unit];
  const minor = 10n ** BigInt(minorDigits);
  const rounded = (numerator * minor + denominator / 2n) / denominator;
  if (rounded === 0n && numerator > 0n)
    return `< ${currency} ${decimalText(1n, minorDigits, false)}`;
  return `${currency} ${decimalText(rounded, minorDigits, false)}`;
}
