<script lang="ts">
	import { Database } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';

	interface Props {
		/** 大きいファイルを探すために、すべてのファイルの表示へ移る */
		onreview: () => void;
	}

	let { onreview }: Props = $props();
</script>

<div
	class="flex align-items:center gap:3 px:4 py:2 bg:bg-subtle bb:1px|solid|state-unsaved flex-shrink:0"
	role="status"
>
	<Database size={18} class="fg:state-unsaved flex-shrink:0" aria-hidden="true" />
	<div class="flex:1 min-w:0 flex flex-direction:column">
		<p class="m:0 type-body">{t('repo_large.message')}</p>
		<p class="m:0 type-small fg:fg-muted">{t('repo_large.detail')}</p>
	</div>
	<Button size="sm" variant="secondary" onclick={onreview}>{t('repo_large.review')}</Button>
</div>
