/** Bound the whole operation, including a native invoke whose own timeout is longer.
 * Settling once prevents abandoned results from updating the caller's UI. */
export async function withDeadline<T>(operation: Promise<T>, milliseconds: number, errorCode: string): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
        return await Promise.race([
            operation,
            new Promise<never>((_, reject) => {
                timer = setTimeout(() => reject(new Error(errorCode)), milliseconds);
            }),
        ]);
    } finally {
        if (timer !== undefined) clearTimeout(timer);
    }
}
