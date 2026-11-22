import java.io.IOException;
import java.net.ServerSocket;
import java.net.Socket;
import java.rmi.server.RMISocketFactory;
import java.util.concurrent.CountDownLatch;

/**
 * TLC for a check on one machine. TLC exports its fingerprint set over RMI
 * even there, which listens on every interface with nobody to answer, so the
 * export gets a socket that is never bound and the check opens none.
 */
public final class LocalTlc {
    private static final class Unbound extends RMISocketFactory {
        @Override
        public Socket createSocket(String host, int port) throws IOException {
            throw new IOException("a local check connects to nobody");
        }

        @Override
        public ServerSocket createServerSocket(int port) throws IOException {
            return new ServerSocket() {
                private final CountDownLatch closed = new CountDownLatch(1);

                @Override
                public int getLocalPort() {
                    return 1;
                }

                @Override
                public Socket accept() throws IOException {
                    try {
                        closed.await();
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                    }
                    throw new IOException("closed");
                }

                @Override
                public void close() {
                    closed.countDown();
                }
            };
        }
    }

    public static void main(String[] args) throws Exception {
        RMISocketFactory.setSocketFactory(new Unbound());
        Class.forName("tlc2.TLC").getMethod("main", String[].class).invoke(null, (Object) args);
    }
}
