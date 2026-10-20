import type { MessageKey } from './index';
import part1 from './en/part1';
import part2 from './en/part2';
import part3 from './en/part3';
import part4 from './en/part4';

// 英語の文言。4 つに分けたものを合成する。キー集合は ja.ts と同じ（型で不足を検出する）
const merged = { ...part1, ...part2, ...part3, ...part4 };

export default merged satisfies Record<MessageKey, string>;
