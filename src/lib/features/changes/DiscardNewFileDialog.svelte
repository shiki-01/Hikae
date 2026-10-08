<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { fileNameOf } from './discard';

	interface Props {
		/** 対象の新規ファイルのパス。null のときは閉じている */
		path: string | null;
		pending: boolean;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let { path, pending, onconfirm, oncancel }: Props = $props();
</script>

<!-- 新規ファイルを「元に戻す（作成しない）」の確認（設計書 4.2、5章の確認ダイアログの 3 要素）。
     取り消せる操作のため、D2 と同じく danger ではなく primary にする -->
<Dialog
	open={path !== null}
	variant="confirm"
	busy={pending}
	title={t('discard.title', { name: path ? fileNameOf(path) : '' })}
	description={t('discard.what')}
	onclose={oncancel}
>
	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('discard.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="secondary" disabled={pending} onclick={oncancel}>{t('discard.cancel')}</Button>
		<Button variant="primary" loading={pending} onclick={onconfirm}>{t('discard.button')}</Button>
	{/snippet}
</Dialog>
