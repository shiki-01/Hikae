<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';

	interface Props {
		/** 未保存のファイルの件数。null のときは閉じている */
		unsavedCount: number | null;
		/** 保存してから取り込む */
		onconfirm: () => void;
		/** 取り込まずに戻る */
		oncancel: () => void;
	}

	let { unsavedCount, onconfirm, oncancel }: Props = $props();
</script>

<!-- 設定「取り込む前に自動で保存する」がオフのときの確認（設計書 5章 E06） -->
<Dialog
	open={unsavedCount !== null}
	variant="confirm"
	title={t('pull_confirm.title')}
	description={t('pull_confirm.what', { count: unsavedCount ?? 0 })}
	onclose={oncancel}
>
	<p class="m:0 type-body">{t('pull_confirm.next')}</p>

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('pull_confirm.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="secondary" onclick={oncancel}>{t('pull_confirm.cancel')}</Button>
		<Button onclick={onconfirm}>{t('pull_confirm.save_and_fetch')}</Button>
	{/snippet}
</Dialog>
