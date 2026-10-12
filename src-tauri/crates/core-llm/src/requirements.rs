// 段階ごとの導入条件の判定（設計 10.2、10.5）。OS の値は呼び出し側が `SystemInfo` として渡す純粋な判定。

use crate::catalog::{Catalog, CatalogEntry};

/// 導入先 PC の状態（呼び出し側が取得した値）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemInfo {
    /// OS が報告する総メモリ（バイト）
    pub total_memory_bytes: u64,
    /// モデルを置くボリュームの空き容量（バイト）
    pub free_disk_bytes: u64,
}

/// 導入を満たせない理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NotEnoughMemory { needed: u64, have: u64 },
    NotEnoughDisk { needed: u64, have: u64 },
}

/// 導入可否。`ok == false` のときは `reason` が必ず入る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Availability {
    pub ok: bool,
    pub reason: Option<Reason>,
}

/// 導入に必要な空き容量。ダウンロードと展開の余裕として、サイズの 1.1 倍とする。
pub fn required_disk_bytes(size_bytes: u64) -> u64 {
    size_bytes.saturating_mul(11) / 10
}

/// モデルを導入できるかを判定する。メモリを先に判定し、足りなければその理由を返す。
pub fn stage_availability(info: SystemInfo, model: &CatalogEntry) -> Availability {
    if info.total_memory_bytes < model.min_total_memory_bytes {
        return Availability {
            ok: false,
            reason: Some(Reason::NotEnoughMemory {
                needed: model.min_total_memory_bytes,
                have: info.total_memory_bytes,
            }),
        };
    }
    let needed = required_disk_bytes(model.size_bytes);
    if info.free_disk_bytes < needed {
        return Availability {
            ok: false,
            reason: Some(Reason::NotEnoughDisk {
                needed,
                have: info.free_disk_bytes,
            }),
        };
    }
    Availability {
        ok: true,
        reason: None,
    }
}

/// 「この PC のおすすめ」を選ぶ段階（段階 2 の中から選ぶ。設計 10.2）
pub const RECOMMENDED_STAGE: u8 = 2;

/// この PC で勧める項目を返す（設計 10.2）。
///
/// `stage` の項目のうち、この PC で導入できるもの（`stage_availability` が通るもの）の中で、
/// 必要メモリ（`min_total_memory_bytes`）が最も大きいものを選ぶ。同じ必要メモリのときは `recommended` の項目を優先する。
/// 導入できる項目が無ければ `None`。
pub fn recommended_for_pc(info: SystemInfo, catalog: &Catalog, stage: u8) -> Option<&CatalogEntry> {
    catalog
        .stage(stage)
        .filter(|m| stage_availability(info, m).ok)
        .max_by_key(|m| (m.min_total_memory_bytes, m.recommended))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn info(total_gib_tenths: u64) -> SystemInfo {
        SystemInfo {
            total_memory_bytes: total_gib_tenths * GIB / 10,
            free_disk_bytes: 100 * GIB,
        }
    }

    #[test]
    fn recommends_e2b_on_an_8gb_class_machine() {
        // 8GB の PC（OS の報告で約 7.6GiB）では、E4B は必要メモリ（12GiB）に足りず導入できない
        let catalog = Catalog::builtin().unwrap();
        let picked = recommended_for_pc(info(76), &catalog, RECOMMENDED_STAGE).unwrap();
        assert_eq!(picked.id, "gemma-4-e2b-it-q4_k_m");
        let e4b = catalog.get("gemma-4-e4b-it-q4_k_m").unwrap();
        assert_eq!(
            stage_availability(info(76), e4b).reason,
            Some(Reason::NotEnoughMemory {
                needed: 12 * GIB,
                have: 7 * GIB + GIB * 6 / 10,
            })
        );
    }

    #[test]
    fn recommends_e4b_from_12gib() {
        let catalog = Catalog::builtin().unwrap();
        let picked = recommended_for_pc(info(120), &catalog, RECOMMENDED_STAGE).unwrap();
        assert_eq!(picked.id, "gemma-4-e4b-it-q4_k_m");
        // 16GB の PC（OS の報告で約 15.7GiB）
        let picked = recommended_for_pc(info(157), &catalog, RECOMMENDED_STAGE).unwrap();
        assert_eq!(picked.id, "gemma-4-e4b-it-q4_k_m");
        // 12GiB 未満（11.9GiB）は E2B
        let picked = recommended_for_pc(info(119), &catalog, RECOMMENDED_STAGE).unwrap();
        assert_eq!(picked.id, "gemma-4-e2b-it-q4_k_m");
    }

    #[test]
    fn no_recommendation_when_nothing_is_available() {
        let catalog = Catalog::builtin().unwrap();
        // メモリが 7GiB に届かない PC では、どの段階 2 も導入できない
        assert!(recommended_for_pc(info(40), &catalog, RECOMMENDED_STAGE).is_none());
        // 空き容量が足りなければ選ばない
        let no_disk = SystemInfo {
            total_memory_bytes: 16 * GIB,
            free_disk_bytes: 0,
        };
        assert!(recommended_for_pc(no_disk, &catalog, RECOMMENDED_STAGE).is_none());
    }

    fn model(size: u64, min_mem: u64) -> CatalogEntry {
        CatalogEntry {
            id: "m".into(),
            display_name: "m".into(),
            stage: 2,
            file_name: "m.gguf".into(),
            url: "https://huggingface.co/a/resolve/main/m.gguf".into(),
            sha256: "0".repeat(64),
            size_bytes: size,
            license_name: "Apache License 2.0".into(),
            license_url: "https://www.apache.org/licenses/LICENSE-2.0".into(),
            min_total_memory_bytes: min_mem,
            recommended: false,
        }
    }

    #[test]
    fn disk_requirement_is_ten_percent_more() {
        assert_eq!(required_disk_bytes(1000), 1100);
        assert_eq!(required_disk_bytes(0), 0);
    }

    #[test]
    fn ok_when_both_are_enough() {
        let m = model(3_000_000_000, 7 * GIB);
        let info = SystemInfo {
            total_memory_bytes: 8 * GIB,
            free_disk_bytes: 4 * GIB,
        };
        assert_eq!(
            stage_availability(info, &m),
            Availability {
                ok: true,
                reason: None
            }
        );
    }

    #[test]
    fn memory_shortage_is_reported_first() {
        let m = model(3_000_000_000, 7 * GIB);
        let info = SystemInfo {
            total_memory_bytes: 4 * GIB,
            free_disk_bytes: 0,
        };
        let a = stage_availability(info, &m);
        assert!(!a.ok);
        assert_eq!(
            a.reason,
            Some(Reason::NotEnoughMemory {
                needed: 7 * GIB,
                have: 4 * GIB
            })
        );
    }

    #[test]
    fn disk_shortage_uses_one_point_one_times_size() {
        let m = model(1000, 0);
        let info = SystemInfo {
            total_memory_bytes: 0,
            free_disk_bytes: 1099,
        };
        assert_eq!(
            stage_availability(info, &m).reason,
            Some(Reason::NotEnoughDisk {
                needed: 1100,
                have: 1099
            })
        );
        let enough = SystemInfo {
            total_memory_bytes: 0,
            free_disk_bytes: 1100,
        };
        assert!(stage_availability(enough, &m).ok);
    }

    #[test]
    fn builtin_models_are_available_on_8gb_class_machine() {
        // OS が 8GB の PC で報告する総メモリ（約 7.6GiB）でも判定が通ること
        let info = SystemInfo {
            total_memory_bytes: 7 * GIB + GIB / 2,
            free_disk_bytes: 10 * GIB,
        };
        let catalog = crate::catalog::Catalog::builtin().unwrap();
        for m in &catalog.models {
            // E4B は必要メモリが 12GiB のため、8GB の PC では導入できない（理由つきで灰色にする）
            if m.id == "gemma-4-e4b-it-q4_k_m" {
                assert!(!stage_availability(info, m).ok, "{}", m.id);
                continue;
            }
            assert!(stage_availability(info, m).ok, "{}", m.id);
        }
    }
}
