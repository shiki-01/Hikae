<script lang="ts">
	import { CloudAlert } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';

	interface Props {
		/** 同期の対象のままでよいと、利用者が選んだ */
		accepted: boolean;
		onkeep: () => void;
		onchange: () => void;
	}

	let { accepted, onkeep, onchange }: Props = $props();
</script>

<!-- 同期フォルダの上のプロジェクトへの注意。拒否はせず、このまま追加するか別のフォルダを選ぶかを利用者が決める -->
<div
	class="flex flex-direction:column gap:2 p:3 r:md bg:bg-subtle b:1px|solid|state-unsaved"
	role="note"
>
	<div class="flex align-items:start gap:2">
		<CloudAlert size={18} class="fg:state-unsaved flex-shrink:0 mt:2px" aria-hidden="true" />
		<p class="m:0 type-body">{t('cloud_sync.message')}</p>
	</div>
	{#if accepted}
		<p class="m:0 type-small fg:fg-muted">{t('cloud_sync.kept')}</p>
	{/if}
	<div class="flex gap:2 justify-content:flex-end">
		<Button size="sm" variant="secondary" onclick={onchange}>{t('cloud_sync.change')}</Button>
		{#if !accepted}
			<Button size="sm" variant="ghost" onclick={onkeep}>{t('cloud_sync.keep')}</Button>
		{/if}
	</div>
</div>
