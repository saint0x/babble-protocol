package util;

import java.io.FileOutputStream;
import java.io.IOException;
import java.security.*;

public class KeyUtil {

  // Method to save RSA private key to file
  public static void savePrivateKeyToFile(PrivateKey privateKey, String filePath) {
    try {
      // Get encoded private key bytes
      byte[] privateKeyBytes = privateKey.getEncoded();
      // Write private key bytes to file
      try (FileOutputStream fos = new FileOutputStream(filePath)) {
        fos.write(privateKeyBytes);
        System.out.println("Private key saved to file: " + filePath);
      }
    } catch (IOException e) {
      System.err.println("Error occurred while saving private key to file: " + e.getMessage());
    }
  }

  // Method to save RSA public key to file
  public static void savePublicKeyToFile(PublicKey publicKey, String filePath) {
    try {
      // Get encoded public key bytes
      byte[] publicKeyBytes = publicKey.getEncoded();
      // Write public key bytes to file
      try (FileOutputStream fos = new FileOutputStream(filePath)) {
        fos.write(publicKeyBytes);
        System.out.println("Public key saved to file: " + filePath);
      }
    } catch (IOException e) {
      System.err.println("Error occurred while saving public key to file: " + e.getMessage());
    }
  }
}
