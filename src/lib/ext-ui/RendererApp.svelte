<script lang="ts">
	// 表示拡張（iframe）の薄いシェル。ホストから `render`・`options` を受け取り、表示の本体へ渡す（設計書 11 章）。
	// 親（ホスト）以外からのメッセージは受け付けない。データ以外（パス・トークン・設定）は届かない。
	import { onMount } from 'svelte';
	import {
		parseOptionsMessage,
		parseRenderMessage,
		RENDERER_PROTOCOL_VERSION,
		type CompareOptions,
		type RenderMessage
	} from '#lib/features/extensions/renderer-protocol.js';
	import RenderCompare from './RenderCompare.svelte';
	import RenderView from './RenderView.svelte';

	let message = $state<RenderMessage | null>(null);
	/** 比較の表示の選択（行差分の並べる・重ねる、強調する箇所）。表示の作り直しを伴わずに変えられる */
	let options = $state<CompareOptions>({ viewMode: 'split', activeBlock: null });

	/** ホストへ送る（origin は opaque のため "*"。内容は表示の結果の種類だけ） */
	function send(event: Record<string, unknown>): void {
		window.parent.postMessage({ v: RENDERER_PROTOCOL_VERSION, ...event }, '*');
	}

	onMount(() => {
		const onMessage = (event: MessageEvent) => {
			if (event.source !== window.parent) return;
			const next = parseRenderMessage(event.data);
			if (next) {
				message = next;
				if (next.mode === 'compare') options = next.options;
				return;
			}
			const change = parseOptionsMessage(event.data);
			if (change && message?.mode === 'compare' && message.id === change.id) {
				options = change.options;
			}
		};
		window.addEventListener('message', onMessage);
		// 読み込みの完了は 1 回だけ知らせる。ホストはこれを待ってから表示の内容を送る
		send({ type: 'ready' });
		return () => window.removeEventListener('message', onMessage);
	});
</script>

<div class="flex flex-direction:column h:100vh w:100% min-h:0 bg:bg fg:fg overflow:hidden">
	{#if message}
		{@const id = message.id}
		{#key id}
			{#if message.mode === 'view'}
				<RenderView
					file={message.file}
					content={message.content}
					onfail={(code) => send({ type: 'error', id, code })}
					onopen={() => send({ type: 'open' })}
				/>
			{:else}
				<RenderCompare
					left={message.left}
					right={message.right}
					diff={message.diff}
					{options}
					onfail={(code) => send({ type: 'error', id, code })}
					onopen={() => send({ type: 'open' })}
				/>
			{/if}
		{/key}
	{/if}
</div>
