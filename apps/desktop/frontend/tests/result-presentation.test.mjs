import assert from 'node:assert/strict';
import test from 'node:test';
import { parseResultData, tableColumns, displayValue, resultLabel } from '../src/lib/result-presentation.ts';

test('formatted data remains exact text, while inspector data becomes structured', () => {
  const source = '{ "answer": 42 }';
  assert.equal(parseResultData(source, 'structured/json', 'arcade.text.structured'), undefined);
  assert.deepEqual(parseResultData(source, 'structured/text-statistics', 'arcade.text.statistics'), { answer: 42 });
  assert.equal(parseResultData('bad JSON', 'structured/anything', 'tool'), undefined);
  assert.equal(parseResultData(source, 'text/plain', 'tool'), undefined);
});

test('large JSON is not parsed for preview; wide and mixed tables fall back to inspection', () => {
  assert.equal(parseResultData(`{"text":"${'x'.repeat(2_000_000)}"}`, 'structured/json', 'tool'), undefined);
  assert.deepEqual(tableColumns([{ first: 1 }, { second: 2 }]), ['first', 'second']);
  assert.deepEqual(tableColumns([{ first: 1 }, null]), []);
  assert.deepEqual(tableColumns([Object.fromEntries(Array.from({ length: 9 }, (_, i) => [`c${i}`, i]))]), []);
});

test('inspection preserves falsy values and labels field names', () => {
  assert.equal(displayValue(false), 'No');
  assert.equal(displayValue(0), '0');
  assert.equal(displayValue(''), '');
  assert.equal(resultLabel('estimatedReadingSeconds'), 'Estimated reading seconds');
});
