import type { StandardToolUi, UiControl } from './contracts';

export type UiValues = Record<string, string>;

function asUiString(value: unknown): string {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return '';
}

export function defaultUiValues(ui: StandardToolUi | null | undefined): UiValues {
  if (!ui || ui.version !== 1) return {};
  const values: UiValues = {};
  for (const control of ui.controls ?? []) {
    const supplied = control.default;
    if (supplied !== undefined) values[control.key] = asUiString(supplied);
    else if (control.type === 'select') values[control.key] = control.choices?.[0]?.value ?? '';
    else if (control.type === 'toggle') values[control.key] = 'false';
    else values[control.key] = '';
  }
  return values;
}

function matchesConditionValue(current: string | undefined, expected: unknown): boolean {
  if (expected === undefined) return false;
  return current === asUiString(expected);
}

export function isUiControlVisible(control: UiControl, values: UiValues): boolean {
  const condition = control.showWhen;
  if (!condition) return true;
  const current = values[condition.key];
  if (condition.equals !== undefined) return matchesConditionValue(current, condition.equals);
  if (condition.oneOf?.length) return condition.oneOf.some((value) => matchesConditionValue(current, value));
  return false;
}

export function standardUiOptionsProblem(ui: StandardToolUi | null | undefined, values: UiValues): string {
  if (!ui) return '';
  if (ui.version !== 1) return `This form uses unsupported UI version ${ui.version}.`;
  for (const control of ui.controls ?? []) {
    if (!isUiControlVisible(control, values)) continue;
    const value = values[control.key] ?? '';
    if (control.type === 'number' && value !== '') {
      const number = Number(value);
      if (!Number.isFinite(number)) return `${control.label} must be a number.`;
      if (control.minimum != null && number < control.minimum) return `${control.label} must be at least ${control.minimum}.`;
      if (control.maximum != null && number > control.maximum) return `${control.label} must be no more than ${control.maximum}.`;
      if (control.step != null) {
        const base = control.minimum ?? 0;
        const steps = (number - base) / control.step;
        if (Math.abs(steps - Math.round(steps)) > 1e-8) return `${control.label} must use steps of ${control.step}.`;
      }
    }
    if (control.type === 'select' && value && control.choices?.length && !control.choices.some((choice) => choice.value === value)) {
      return `Choose a valid ${control.label.toLowerCase()}.`;
    }
    if (control.type === 'toggle' && value !== 'true' && value !== 'false') return `${control.label} must be on or off.`;
  }
  return '';
}

export function serializeStandardUiOptions(ui: StandardToolUi | null | undefined, values: UiValues): Record<string, unknown> {
  if (!ui || ui.version !== 1) return {};
  const options: Record<string, unknown> = {};
  for (const control of ui.controls ?? []) {
    if (!isUiControlVisible(control, values)) continue;
    const value = values[control.key] ?? '';
    if (control.type === 'toggle') {
      options[control.key] = value === 'true';
    } else if (control.type === 'number') {
      if (value !== '') options[control.key] = Number(value);
    } else if (value !== '') {
      options[control.key] = value;
    }
  }
  return options;
}
