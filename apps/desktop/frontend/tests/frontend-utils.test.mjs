import assert from 'node:assert/strict';
import test from 'node:test';

import { acceptsSelectedFile, acceptsTextInput, inputMime, usesFileInput } from '../src/lib/tool-utils.ts';
import { defaultUiValues, serializeStandardUiOptions, standardUiOptionsProblem } from '../src/lib/standard-ui.ts';

const selectedFile = (mime) => ({ token: 'grant-1', name: 'input.bin', size: 1, mime });

test('file array input declarations accept each compatible selected file', () => {
  const tool = { id: 'test.any', inputs: ['file/any[]'] };
  assert.equal(acceptsSelectedFile(tool, selectedFile('file/image')), true);

  const mediaTool = { id: 'test.media', inputs: ['file/media[]'] };
  assert.equal(acceptsSelectedFile(mediaTool, selectedFile('file/video')), true);
  assert.equal(acceptsSelectedFile(mediaTool, selectedFile('file/audio')), true);
  assert.equal(acceptsSelectedFile(mediaTool, selectedFile('file/pdf')), false);
});

test('array declarations preserve MIME wildcard matching', () => {
  const tool = { id: 'test.images', inputs: ['file/image/*[]'] };
  assert.equal(acceptsSelectedFile(tool, selectedFile('file/image/png')), true);
  assert.equal(acceptsSelectedFile(tool, selectedFile('file/audio')), false);
});

test('an explicit text or URL UI input takes precedence over file MIME alternatives', () => {
  const textTool = {
    id: 'test.dual-text',
    inputs: ['text/plain', 'file/any'],
    ui: { version: 1, input: { kind: 'text', label: 'Text' }, controls: [] },
  };
  assert.equal(usesFileInput(textTool), false);
  assert.equal(acceptsTextInput(textTool), true);
  assert.equal(inputMime(textTool), 'text/plain');

  const urlTool = {
    id: 'test.dual-url',
    inputs: ['text/url', 'file/any'],
    ui: { version: 1, input: { kind: 'url', label: 'URL' }, controls: [] },
  };
  assert.equal(usesFileInput(urlTool), false);
  assert.equal(acceptsTextInput(urlTool), true);
  assert.equal(inputMime(urlTool), 'text/url');
});

test('an explicit file input remains file based even when text is also accepted', () => {
  const tool = {
    id: 'test.file-first',
    inputs: ['text/plain', 'file/any'],
    ui: { version: 1, input: { kind: 'file', label: 'Text or file' }, controls: [] },
  };
  assert.equal(usesFileInput(tool), true);
  assert.equal(acceptsTextInput(tool), false);
});

test('standard UI defaults validate and serialize only visible controls', () => {
  const ui = {
    version: 1,
    input: { kind: 'text', label: 'Text' },
    controls: [
      { key: 'format', label: 'Format', type: 'select', default: 'json', choices: [{ value: 'json', label: 'JSON' }, { value: 'yaml', label: 'YAML' }] },
      { key: 'enabled', label: 'Enabled', type: 'toggle' },
      { key: 'indent', label: 'Indent', type: 'number', default: 2, minimum: 1, maximum: 8, step: 1 },
      { key: 'yamlOption', label: 'YAML option', type: 'text', default: 'ignored', showWhen: { key: 'format', equals: 'yaml' } },
    ],
  };

  const values = defaultUiValues(ui);
  assert.deepEqual(values, { format: 'json', enabled: 'false', indent: '2', yamlOption: 'ignored' });
  assert.equal(standardUiOptionsProblem(ui, values), '');
  assert.deepEqual(serializeStandardUiOptions(ui, values), { format: 'json', enabled: false, indent: 2 });

  values.indent = '9';
  assert.equal(standardUiOptionsProblem(ui, values), 'Indent must be no more than 8.');
  values.indent = '2';
  values.format = 'yaml';
  values.yamlOption = 'shown';
  assert.deepEqual(serializeStandardUiOptions(ui, values), { format: 'yaml', enabled: false, indent: 2, yamlOption: 'shown' });
});
