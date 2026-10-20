<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';

	// テスト用: テンプレート内の t() と日時の表示が、言語の切り替えに追随するかを確かめる
	interface Props {
		at: Date;
	}

	let { at }: Props = $props();
</script>

<p data-testid="name">{t('app.name')}</p>
<p data-testid="datetime">{formatDateTime(at)}</p>
