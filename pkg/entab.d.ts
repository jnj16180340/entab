/* tslint:disable */
/* eslint-disable */
/**
*/
export function start(): void;
/**
*/
export class Reader {
  free(): void;
/**
* @param {Uint8Array} data
* @param {string | undefined} parser
*/
  constructor(data: Uint8Array, parser?: string);
/**
* @returns {any}
*/
  next(): any;
/**
* @returns {any}
*/
  readonly headers: any;
/**
* @returns {any}
*/
  readonly metadata: any;
/**
* @returns {string}
*/
  readonly parser: string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_reader_free: (a: number) => void;
  readonly reader_new: (a: number, b: number, c: number, d: number, e: number) => void;
  readonly reader_parser: (a: number, b: number) => void;
  readonly reader_headers: (a: number) => number;
  readonly reader_metadata: (a: number, b: number) => void;
  readonly reader_next: (a: number, b: number) => void;
  readonly start: () => void;
  readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
  readonly __wbindgen_malloc: (a: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number) => number;
  readonly __wbindgen_free: (a: number, b: number) => void;
  readonly __wbindgen_start: () => void;
}

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {InitInput | Promise<InitInput>} module_or_path
*
* @returns {Promise<InitOutput>}
*/
export default function init (module_or_path?: InitInput | Promise<InitInput>): Promise<InitOutput>;
