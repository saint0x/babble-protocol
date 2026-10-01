import model.Transaction;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.*;
import java.security.spec.InvalidKeySpecException;
import java.security.spec.PKCS8EncodedKeySpec;
import java.util.ArrayList;
import java.util.List;

public class LedgerService {
  // Define a list to simulate the immutable ledger
  private List<Transaction> ledger;

  // Constructor
  public LedgerService() {
    this.ledger = new ArrayList<>();
  }

  // Method to submit a transaction to the ledger
  public void submitTransaction(Transaction transaction, PrivateKey privateKey) {
    // Add the transaction to the ledger
    transaction.setTimestamp(System.currentTimeMillis()); // Set current timestamp
    transaction.setSignature(signTransaction(transaction, privateKey)); // Sign the transaction with the provided
                                                                        // private key
    ledger.add(transaction);
    // Print confirmation message
    System.out.println("Transaction submitted to the ledger: " + transaction.getTransactionId());
  }

  // Method to query transaction history from the ledger for a specific user
  public List<Transaction> queryTransactionHistory(String userId) {
    List<Transaction> userTransactions = new ArrayList<>();
    // Iterate through ledger to find transactions related to the user
    for (Transaction transaction : ledger) {
      if (transaction.getFromUserId().equals(userId) || transaction.getToUserId().equals(userId)) {
        userTransactions.add(transaction);
      }
    }
    // Print transaction history
    System.out.println("Transaction history for user " + userId + ":");
    for (Transaction transaction : userTransactions) {
      System.out.println(transaction.getTransactionId() + " - " + transaction.getContent());
    }
    return userTransactions;
  }

  // Method to sign a transaction using cryptographic signatures
  private String signTransaction(Transaction transaction, PrivateKey privateKey) {
      try {
          Signature signature = Signature.getInstance("SHA256withRSA");
          signature.initSign(privateKey);
          signature.update(transaction.toString().getBytes());
          byte[] signatureBytes = signature.sign();
          return bytesToHex(signatureBytes); // Convert signature to hexadecimal string
      } catch (NoSuchAlgorithmException | InvalidKeyException | SignatureException e) {
          e.printStackTrace();
          return null;
      }
  }


  // Method to convert byte array to hexadecimal string
  private String bytesToHex(byte[] bytes) {
    StringBuilder result = new StringBuilder();
    for (byte b : bytes) {
      result.append(String.format("%02x", b));
    }
    return result.toString();
  }

  // Method to get the private key from file in the root directory
  private PrivateKey getPrivateKey() {
    try {
      byte[] keyBytes = Files.readAllBytes(Paths.get("private_key.der"));
      KeyFactory keyFactory = KeyFactory.getInstance("RSA");
      return keyFactory.generatePrivate(new PKCS8EncodedKeySpec(keyBytes));
    } catch (Exception e) {
      e.printStackTrace();
      return null;
    }
  }

}
