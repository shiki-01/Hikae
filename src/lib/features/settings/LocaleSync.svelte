<script lang="ts">
	import { setLocale } from '#lib/i18n/index.js';
	import { useSettings } from './queries';

	// 起動時の設定の読み込みと、設定の変更（楽観的な更新を含む）に合わせて画面の言語を決める（設計書 2.3）。
	// 保存に失敗して設定が戻ったときも、この効果で元の言語に戻る
	const settings = useSettings(() => null);

	$effect(() => {
		const language = settings.data?.settings.language;
		if (language) setLocale(language);
	});
</script>
