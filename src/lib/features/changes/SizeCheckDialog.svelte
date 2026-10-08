<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatBytes } from '#lib/i18n/format.js';
	import type { SizeCheck } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { viewSizeCheck, type SizeAction } from './size-check';

	interface Props {
		/** 大きいファイルの検査結果。null のときは閉じている */
		check: SizeCheck | null;
		/** 選択後に保存をやり直している間は true */
		pending: boolean;
		onchoose: (action: SizeAction) => void;
		oncancel: () => void;
	}

	let { check, pending, onchoose, oncancel }: Props = $props();

	const view = $derived(check ? viewSizeCheck(check) : null);
</script>

<Dialog
	open={check !== null}
	variant="warning"
	title={t('size_check.title')}
	description={view?.hasBlocked
		? t('size_check.description_blocked')
		: t('size_check.description_warned')}
	busy={pending}
	onclose={oncancel}
>
	{#if view}
		<div class="flex flex-direction:column gap:3">
			<ul
				class="m:0 p:0 list-style:none b:1px|solid|border r:md"
				aria-label={t('size_check.list_label')}
			>
				{#each view.rows as row (row.path)}
					<li class="flex flex-direction:column gap:1 px:3 py:2 bb:1px|solid|border">
						<span class="type-body overflow-wrap:anywhere">{row.label}</span>
						<span class="type-small fg:fg-muted">
							{row.sizeBytes === null ? t('size_check.size_unknown') : formatBytes(row.sizeBytes)}
							・
							<span class={row.blocked ? 'fg:state-danger' : 'fg:state-unsaved'}>
								{row.blocked ? t('size_check.tag_blocked') : t('size_check.tag_warned')}
							</span>
						</span>
					</li>
				{/each}
			</ul>
			<p class="m:0 type-small fg:fg-muted">{t('size_check.next')}</p>
		</div>
	{/if}

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('size_check.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<div class="flex flex-flow:row|wrap justify-content:end gap:3 flex:1">
			<Button variant="ghost" disabled={pending} onclick={oncancel}>
				{t('size_check.cancel')}
			</Button>
			{#if view?.canAccept}
				<Button variant="secondary" disabled={pending} onclick={() => onchoose('accept')}>
					{t('size_check.accept')}
				</Button>
			{/if}
			<Button
				variant="primary"
				loading={pending}
				disabled={pending}
				onclick={() => onchoose('exclude')}
			>
				{view && view.rows.length > 1 ? t('size_check.exclude_many') : t('size_check.exclude_one')}
			</Button>
		</div>
	{/snippet}
</Dialog>
