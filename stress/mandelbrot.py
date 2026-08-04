import multiprocessing as mp
import signal
import sys

WIDTH = 4000
HEIGHT = 4000
MAX_ITER = 1000
CORES = 20

X_MIN, X_MAX = -2.0, 0.5
Y_MIN, Y_MAX = -1.25, 1.25


def init_worker():
    """Informa i processi figli di ignorare SIGINT: ci penserà il padre."""
    signal.signal(signal.SIGINT, signal.SIG_IGN)


def compute_row(y_idx):
    cy = Y_MIN + (y_idx / HEIGHT) * (Y_MAX - Y_MIN)
    row = [0] * WIDTH

    for x_idx in range(WIDTH):
        cx = X_MIN + (x_idx / WIDTH) * (X_MAX - X_MIN)
        c = complex(cx, cy)
        z = 0j
        n = 0

        while abs(z) <= 2.0 and n < MAX_ITER:
            z = z * z + c
            n += 1

        row[x_idx] = n

    return row


def main():
    # Passiamo initializer=init_worker al pool
    pool = mp.Pool(processes=CORES, initializer=init_worker)

    try:
        # Usiamo map_async con un timeout molto lungo (es. 999999 secondi)
        # Questo permette a Python di intercettare il Ctrl+C (KeyboardInterrupt)
        async_res = pool.map_async(compute_row, range(HEIGHT))
        result_matrix = async_res.get(timeout=999999)
        
        pool.close()
        pool.join()
        return result_matrix

    except KeyboardInterrupt:
        print("\n[!] Interruzione da tastiera rilevata. Arresto dei 20 processi...")
        pool.terminate()  # Forza l'uccisione immediata dei processi worker
        pool.join()
        sys.exit(1)


if __name__ == "__main__":
    matrix = main()