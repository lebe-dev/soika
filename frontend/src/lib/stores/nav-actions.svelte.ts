let _newProjectCallback = $state<(() => void) | null>(null);

export const navActions = {
  get newProjectCallback() {
    return _newProjectCallback;
  },
  set newProjectCallback(cb: (() => void) | null) {
    _newProjectCallback = cb;
  }
};
