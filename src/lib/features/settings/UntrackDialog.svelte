<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { summarizeTracked } from './ignore-model';

	interface Props {
		/** 保存対象から外すファイルのパス。null のときは閉じている */
		files: string[] | null;
		/** 保存の処理中は true */
		busy: boolean;
		onsubmit: (paths: string[]) => void;
		oncancel: () => void;
	}

	let { files, busy, onsubmit, oncancel }: Props = $props();

	const summary = $derived(files ? summarizeTracked(files) : null);
</script>

<Dialog
	open={files !== null}
	variant="warning"
	title={t('ignore.untrack_title')}
	description={t('ignore.untrack_description')}
	{busy}
	onclose={oncancel}
>
	{#if summary}
		<div class="flex flex-direction:column gap:2">
			<ul
				class="m:0 p:0 list-style:none b:1px|solid|border r:md"
				aria-label={t('ignore.list_label')}
			>
				{#each summary.shown as path (path)}
					<li class="type-body px:3 py:2 bb:1px|solid|border overflow-wrap:anywhere">{path}</li>
				{/each}
			</ul>
			{#if summary.rest > 0}
				<p class="m:0 type-small fg:fg-muted">
					{t('ignore.untrack_more', { count: summary.rest })}
				</p>
			{/if}
		</div>
	{/if}

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('ignore.untrack_safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<div class="flex flex-flow:row|wrap justify-content:end gap:3 flex:1">
			<Button variant="ghost" disabled={busy} onclick={oncancel}>{t('ignore.cancel')}</Button>
			<Button
				variant="primary"
				loading={busy}
				disabled={busy}
				onclick={() => {
					if (files) onsubmit(files);
				}}
			>
				{t('ignore.untrack_submit')}
			</Button>
		</div>
	{/snippet}
</Dialog>
