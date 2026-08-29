import { useId } from 'react';

import { channelsToHex, hexToChannels } from '../../../../lib/theme/color';
import Field from '../../../ui/Field';

interface ColorTokenFieldProps {
  /** Token key without `--` (e.g. `surface-canvas`). */
  tokenKey: string;
  /** Human label for the swatch row. */
  label: string;
  /** Current value as a `"R G B"` channel triple (effective or override). */
  value: string;
  /** Whether this field is editable (false when a built-in theme is active). */
  disabled?: boolean;
  /** Called with the new `"R G B"` channel triple. */
  onChange: (channels: string) => void;
}

/**
 * A single editable colour token row: a native colour swatch + read-only hex
 * label. Converts between the stored channel format and the hex the native
 * `<input type="color">` speaks.
 */
const ColorTokenField = ({ tokenKey, label, value, disabled, onChange }: ColorTokenFieldProps) => {
  const id = useId();
  const hex = channelsToHex(value);

  return (
    <Field
      htmlFor={id}
      label={label}
      description={`--${tokenKey} · ${hex}`}
      className="px-0 py-1.5"
      control={
        <input
          id={id}
          type="color"
          value={hex}
          disabled={disabled}
          onChange={e => onChange(hexToChannels(e.target.value))}
          aria-label={label}
          className="h-8 w-12 shrink-0 cursor-pointer rounded-md border border-line bg-surface disabled:cursor-not-allowed disabled:opacity-50"
        />
      }
    />
  );
};

export default ColorTokenField;
