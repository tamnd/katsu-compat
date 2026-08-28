'use strict';
// Flags: --no-warnings
const assert = require('node:assert');
assert.ok(process.execArgv.includes('--no-warnings'), 'the Flags directive must be honoured');
