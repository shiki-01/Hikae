<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatBytes } from '#lib/i18n/format.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { viewSizeCheck } from '#lib/features/changes/size-check.js';
	import type { SyncSizeRequest } from './sync-size';

	interface Props {
		/** 取り込み・アップロードを見送った理由。null のときは閉じている */
		request: SyncSizeRequest | null;
		onclose: () => void;
	}

	let { request, onclose }: Props = $props();

	const view = $derived(request ? viewSizeCheck(request.check) : null);
</script>

<Dialog
	open={request !== null}
	variant="warning"
	title={request?.kind === 'push' ? t('sync_size.title_push') : t('sync_size.title_fetch')}
	description={request?.kind === 'push'
		? t('sync_size.description_push')
		: t('sync_size.description_fetch')}
	{onclose}
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
			<p class="m:0 type-body">{t('sync_size.next')}</p>
		</div>
	{/if}

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('sync_size.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button onclick={onclose}>{t('sync_size.close')}</Button>
	{/snippet}
</Dialog>
