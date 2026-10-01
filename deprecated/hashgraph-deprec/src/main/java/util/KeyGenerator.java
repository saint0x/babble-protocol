package util;

import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.NoSuchAlgorithmException;

public class KeyGenerator {

    // Method to generate RSA key pair
    public static KeyPair generateRSAKeyPair() {
        try {
            // Initialize RSA key pair generator with 2048 bits key size
            KeyPairGenerator keyGen = KeyPairGenerator.getInstance("RSA");
            keyGen.initialize(2048);
            // Generate key pair
            return keyGen.generateKeyPair();
        } catch (NoSuchAlgorithmException e) {
            // Handle exception if RSA algorithm is not available
            e.printStackTrace();
            return null;
        }
    }
}
