import type {Plugin} from 'vite';
export function mockPlugin():Plugin;
export function answer(command:string,args?:Record<string,unknown>,scenario?:string):Promise<unknown>;
export const calls:unknown[];
