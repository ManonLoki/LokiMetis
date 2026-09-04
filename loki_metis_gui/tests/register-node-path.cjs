"use strict";

const Module = require("node:module");
const path = require("node:path");

process.env.NODE_PATH = path.resolve(__dirname, "../node_modules");
Module.Module._initPaths();
