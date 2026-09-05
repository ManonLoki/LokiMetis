import { useState } from 'react';

/** 保存与业务键绑定的本地草稿，避免切换对象时串值。 */
interface Draft<Key, Value> {
  baseline: Key;
  value: Value;
}

/**
 * 维护一个跟随服务端基线自动失效的本地编辑草稿：只要 `savedKey` 未变化，
 * 用户的编辑就保留；一旦服务端基线变化（如另一处保存成功后重新拉取），
 * 尚未提交的草稿自动回退为按最新基线派生的值。
 *
 * `setDraft` 把新值与当前基线配对，标记草稿“正在偏离，等待该基线被确认”；
 * `resetTo` 把新基线与新值一起写入，用于保存成功后把草稿钉死在已知的服务端结果上，
 * 不依赖调用方 props 是否已经用最新查询结果重新渲染。
 */
export function useDraftValue<Key, Value>(
  savedKey: Key,
  computeFromSaved: () => Value,
): {
  value: Value;
  setDraft: (value: Value) => void;
  resetTo: (key: Key, value: Value) => void;
} {
  const [draft, setDraftState] = useState<Draft<Key, Value>>(() => ({
    baseline: savedKey,
    value: computeFromSaved(),
  }));
  const value = draft.baseline === savedKey ? draft.value : computeFromSaved();
  return {
    value,
    setDraft: (next: Value) => setDraftState({ baseline: savedKey, value: next }),
    resetTo: (key: Key, next: Value) => setDraftState({ baseline: key, value: next }),
  };
}
