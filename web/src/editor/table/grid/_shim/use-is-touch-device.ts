import { useMediaQuery } from '@kition/kit-editor'

export const useIsTouchDevice = (): boolean => useMediaQuery('(pointer: coarse)')
